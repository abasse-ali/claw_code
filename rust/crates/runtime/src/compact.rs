use crate::session::{ContentBlock, ConversationMessage, MessageRole, Session};

const COMPACT_CONTINUATION_PREAMBLE: &str =
    "This session is being continued from a previous conversation that ran out of context. The summary below covers the earlier portion of the conversation.\n\n";
const COMPACT_RECENT_MESSAGES_NOTE: &str = "Recent messages are preserved verbatim.";
const COMPACT_DIRECT_RESUME_INSTRUCTION: &str = "Continue the conversation from where it left off without asking the user any further questions. Resume directly - do not acknowledge the summary, do not recap what was happening, and do not preface with continuation text.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompactionConfig {
    pub preserve_recent_messages: usize,
    pub max_estimated_tokens: usize,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            preserve_recent_messages: 4,
            max_estimated_tokens: 10_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactionResult {
    pub summary: String,
    pub formatted_summary: String,
    pub compacted_session: Session,
    pub removed_message_count: usize,
    /// Le résumé est celui du modèle (sinon c'est celui de Claw).
    pub model_summary: bool,
}

#[must_use]
pub fn estimate_session_tokens(session: &Session) -> usize {
    session.messages.iter().map(estimate_message_tokens).sum()
}

#[must_use]
pub fn should_compact(session: &Session, config: CompactionConfig) -> bool {
    let start = compacted_summary_prefix_len(session);
    let compactable = &session.messages[start..];

    compactable.len() > config.preserve_recent_messages
        && compactable
            .iter()
            .map(estimate_message_tokens)
            .sum::<usize>()
            >= config.max_estimated_tokens
}

#[must_use]
pub fn format_compact_summary(summary: &str) -> String {
    let without_analysis = strip_tag_block(summary, "analysis");
    let formatted = if let Some(content) = extract_tag_block(&without_analysis, "summary") {
        without_analysis.replace(
            &format!("<summary>{content}</summary>"),
            &format!("Summary:\n{}", content.trim()),
        )
    } else {
        without_analysis
    };

    collapse_blank_lines(&formatted).trim().to_string()
}

#[must_use]
pub fn get_compact_continuation_message(
    summary: &str,
    suppress_follow_up_questions: bool,
    recent_messages_preserved: bool,
) -> String {
    compact_continuation(
        summary,
        "",
        suppress_follow_up_questions,
        recent_messages_preserved,
    )
}

/// Message qui ouvre la session compactée : le résumé mis en forme, puis les
/// faits notés par Claw, tels quels (ni balises ni lignes vides retouchées
/// dans les demandes citées), puis les consignes de reprise.
fn compact_continuation(
    summary: &str,
    facts: &str,
    suppress_follow_up_questions: bool,
    recent_messages_preserved: bool,
) -> String {
    let mut base = format!(
        "{COMPACT_CONTINUATION_PREAMBLE}{}",
        with_facts(&format_compact_summary(summary), facts)
    );

    if recent_messages_preserved {
        base.push_str("\n\n");
        base.push_str(COMPACT_RECENT_MESSAGES_NOTE);
    }

    if suppress_follow_up_questions {
        base.push('\n');
        base.push_str(COMPACT_DIRECT_RESUME_INSTRUCTION);
    }

    base
}

/// Consigne donnée au modèle pour résumer la partie compactée d'une conversation.
pub const MODEL_SUMMARY_PROMPT: &str = "You are compacting the conversation of a coding agent so that the work can continue in a fresh context window. Write a structured summary that lets the agent resume seamlessly. Include: (1) the user's requests and explicit instructions, quoting the important ones verbatim; (2) key technical context and decisions made; (3) files read, created or modified, with what changed and why (exact paths and symbol names); (4) commands run and their outcome, including build and test results and exact error messages; (5) problems solved and issues still open; (6) the task currently in progress and the next concrete steps. Be specific and factual, skip pleasantries, write in the language the user uses, and output only the summary.";

const TRANSCRIPT_TEXT_LIMIT: usize = 6_000;
const TRANSCRIPT_TOOL_INPUT_LIMIT: usize = 2_000;
const TRANSCRIPT_TOOL_OUTPUT_LIMIT: usize = 2_500;
const TRANSCRIPT_TOTAL_LIMIT: usize = 150_000;
const TRANSCRIPT_HEAD_KEPT: usize = 30_000;

#[must_use]
pub fn compact_session(session: &Session, config: CompactionConfig) -> CompactionResult {
    compact_session_with(session, config, |_| None)
}

/// Compacte la session ; `summarize` reçoit les messages à résumer (y compris un
/// éventuel résumé précédent) et peut renvoyer un résumé rédigé par le modèle.
/// À défaut, un résumé local est construit.
pub fn compact_session_with(
    session: &Session,
    config: CompactionConfig,
    summarize: impl FnOnce(&[ConversationMessage]) -> Option<String>,
) -> CompactionResult {
    if !should_compact(session, config) {
        return CompactionResult {
            summary: String::new(),
            formatted_summary: String::new(),
            compacted_session: session.clone(),
            removed_message_count: 0,
            model_summary: false,
        };
    }

    let existing_summary = session
        .messages
        .first()
        .and_then(extract_existing_compacted_summary);
    let compacted_prefix_len = usize::from(existing_summary.is_some());
    let keep_from = safe_keep_from(
        &session.messages,
        session
            .messages
            .len()
            .saturating_sub(config.preserve_recent_messages),
        compacted_prefix_len,
    );
    if keep_from <= compacted_prefix_len {
        return CompactionResult {
            summary: String::new(),
            formatted_summary: String::new(),
            compacted_session: session.clone(),
            removed_message_count: 0,
            model_summary: false,
        };
    }
    let removed = &session.messages[compacted_prefix_len..keep_from];
    let preserved = session.messages[keep_from..].to_vec();
    // Ce que Claw sait lui-même, mot pour mot : les demandes de l'utilisateur
    // (celles d'une compaction précédente comprises) et les fichiers modifiés.
    // Le résumé ne les perd ainsi jamais, même rédigé par un petit modèle.
    let facts = render_facts(&claw_facts(existing_summary.as_deref(), removed));
    // Un texte qui n'a rien d'un résumé (le modèle a continué la conversation
    // au lieu de la résumer) est écarté : le résumé de Claw le remplace.
    let model_summary = summarize(&session.messages[..keep_from])
        .map(|text| clean_model_summary(&text))
        .filter(|text| summary_is_usable(text, removed));
    let summarized_by_model = model_summary.is_some();
    let summary = if let Some(text) = model_summary {
        format!("<summary>\n{text}\n</summary>")
    } else {
        let earlier = existing_summary.as_deref().map(without_facts);
        merge_compact_summaries(earlier.as_deref(), &summarize_messages(removed))
    };
    let formatted_summary = with_facts(&format_compact_summary(&summary), &facts);
    let continuation = compact_continuation(&summary, &facts, true, !preserved.is_empty());

    let mut compacted_messages = vec![ConversationMessage {
        role: MessageRole::System,
        blocks: vec![ContentBlock::Text { text: continuation }],
        usage: None,
    }];
    compacted_messages.extend(preserved);

    CompactionResult {
        summary: with_facts(&summary, &facts),
        formatted_summary,
        compacted_session: Session {
            version: session.version,
            messages: compacted_messages,
        },
        removed_message_count: removed.len(),
        model_summary: summarized_by_model,
    }
}

/// Rappel placé après la conversation à résumer : sans lui, un petit modèle
/// voit une conversation et en écrit le tour suivant.
const SUMMARY_END_REMINDER: &str = "[End of the conversation to summarize. Do not continue it and do not answer the user. Write the summary now, under these headings: 1. User requests (verbatim) 2. Decisions 3. Files changed 4. Commands and results 5. Open problems 6. Next step.]";

/// Ajouté à la conversation pour la seconde demande de résumé, quand la
/// première réponse n'en était pas un.
pub const SUMMARY_RETRY_NOTE: &str = "[Your previous reply continued the conversation instead of summarizing it. Do not continue the conversation and do not answer the user: write only the summary, under the six headings above, and name the files that were changed.]";

/// Rubriques d'un résumé, en anglais ou en français : un résumé utilisable en
/// porte au moins deux en titre.
const SUMMARY_RUBRICS: [&[&str]; 6] = [
    &[
        "request",
        "demande",
        "instruction",
        "requirement",
        "exigence",
        "objective",
        "objectif",
        "goal",
    ],
    &[
        "decision",
        "décision",
        "context",
        "contexte",
        "approach",
        "approche",
    ],
    &["file", "fichier", "code change", "modification"],
    &["command", "commande"],
    &[
        "problem",
        "problème",
        "issue",
        "error",
        "erreur",
        "bug",
        "blocage",
    ],
    &[
        "next step",
        "prochaine",
        "étape",
        "current",
        "en cours",
        "in progress",
        "pending",
        "à faire",
        "remaining",
    ],
];

/// Le texte rendu par le modèle chargé de résumer en est bien un : il porte
/// au moins trois des rubriques demandées en titre et, si des fichiers ont été
/// modifiés dans la partie résumée, il en nomme au moins un. Le texte est lu
/// comme il serait gardé (sans balises, raisonnement ni faits recopiés).
#[must_use]
pub fn summary_is_usable(text: &str, summarized: &[ConversationMessage]) -> bool {
    let text = clean_model_summary(text);
    let mut in_code = false;
    let headings: Vec<String> = text
        .lines()
        .filter_map(|line| {
            if line.trim_start().starts_with("```") {
                in_code = !in_code;
                return None;
            }
            if in_code {
                None
            } else {
                heading(line)
            }
        })
        .collect();
    let rubrics = SUMMARY_RUBRICS
        .iter()
        .filter(|words| {
            headings
                .iter()
                .any(|title| words.iter().any(|word| starts_a_word(title, word)))
        })
        .count();
    let files = changed_files(summarized);
    let names_a_file = files.is_empty()
        || files.iter().any(|file| {
            let name = file.rsplit(['/', '\\']).next().unwrap_or(file);
            !name.is_empty() && text.contains(name)
        });
    rubrics >= 3 && names_a_file
}

/// `word` commence un mot de `title` : « file » dans « files », pas dans
/// « profile » ; « command » pas dans « recommandation ».
fn starts_a_word(title: &str, word: &str) -> bool {
    title.match_indices(word).any(|(at, _)| {
        title[..at]
            .chars()
            .next_back()
            .is_none_or(|before| !before.is_alphanumeric())
    })
}

/// Le texte du modèle sans ce qui n'est pas son résumé : balises, raisonnement
/// (`<analysis>`), faits de Claw qu'il aurait recopiés.
fn clean_model_summary(text: &str) -> String {
    let text = strip_tag_block(text, "analysis");
    without_facts(&text.replace("<summary>", "").replace("</summary>", ""))
        .trim()
        .to_string()
}

/// Le titre que porte cette ligne, en minuscules, si elle en a l'air : titre
/// Markdown ou texte en gras, ou quelques mots numérotés ou suivis de
/// deux-points.
fn heading(line: &str) -> Option<String> {
    let line = line.trim();
    let title = line.split_once(':').map_or(line, |(title, _)| title);
    let words = title
        .split_whitespace()
        .filter(|word| word.chars().any(char::is_alphabetic))
        .count();
    let unnumbered =
        line.trim_start_matches(|c: char| c.is_ascii_digit() || matches!(c, '.' | ')' | '-' | ' '));
    let marked = line.starts_with('#') || unnumbered.starts_with("**");
    let numbered = unnumbered.len() < line.len() && line.starts_with(|c: char| c.is_ascii_digit());
    let titled = (marked && (1..=12).contains(&words))
        || ((numbered || line.contains(':')) && (1..=6).contains(&words));
    titled.then(|| title.to_lowercase())
}

/// Outils qui écrivent un fichier.
const WRITING_TOOLS: [&str; 3] = ["write_file", "edit_file", "NotebookEdit"];

/// Les fichiers écrits ou modifiés par les appels réussis de ces messages,
/// dans l'ordre des appels.
fn changed_files(messages: &[ConversationMessage]) -> Vec<String> {
    let succeeded: Vec<&str> = messages
        .iter()
        .flat_map(|message| &message.blocks)
        .filter_map(|block| match block {
            ContentBlock::ToolResult {
                tool_use_id,
                is_error: false,
                ..
            } => Some(tool_use_id.as_str()),
            _ => None,
        })
        .collect();
    messages
        .iter()
        .flat_map(|message| &message.blocks)
        .filter_map(|block| match block {
            ContentBlock::ToolUse { id, name, input }
                if WRITING_TOOLS.contains(&name.as_str()) && succeeded.contains(&id.as_str()) =>
            {
                let value: serde_json::Value = serde_json::from_str(input).ok()?;
                ["path", "file_path", "filePath", "notebook_path"]
                    .iter()
                    .find_map(|key| value.get(*key)?.as_str())
                    .map(str::to_string)
            }
            _ => None,
        })
        .collect()
}

/// Début et fin des faits que Claw note lui-même sous un résumé, et leurs
/// rubriques.
const FACTS_START: &str = "[Recorded by Claw, exact]";
const FACTS_END: &str = "[End of what Claw recorded]";
const FACTS_REQUESTS: &str = "User requests, word for word, oldest first:";
const FACTS_FILES: &str = "Files written or edited:";
/// Mis à la place des demandes du milieu quand elles ne tiennent plus.
const OMITTED_REQUESTS: &str = "(… older requests left out for space …)";
/// Place des demandes de l'utilisateur dans ces faits, en caractères ; celle
/// de la première (la demande d'origine), puis de chacune des autres.
const FACTS_REQUESTS_LIMIT: usize = 8_000;
const FACTS_FIRST_REQUEST_LIMIT: usize = 5_000;
const FACTS_REQUEST_LIMIT: usize = 2_000;
/// Fichiers cités au plus (les plus récents).
const FACTS_FILES_LIMIT: usize = 40;

/// Ce que Claw sait lui-même de la partie résumée d'une conversation.
#[derive(Debug, Default)]
struct Facts {
    requests: Vec<String>,
    files: Vec<String>,
}

/// Les faits de Claw pour cette compaction : ceux de la compaction
/// précédente, puis ceux des messages retirés.
fn claw_facts(earlier_summary: Option<&str>, removed: &[ConversationMessage]) -> Facts {
    let mut facts = earlier_summary.map(recorded_facts).unwrap_or_default();
    facts.requests.extend(user_requests(removed));
    facts.files.extend(changed_files(removed));
    Facts {
        requests: kept_requests(facts.requests),
        files: kept_files(facts.files),
    }
}

/// Message de l'utilisateur écrit par Claw (remarque, relance d'un hook
/// `Stop`) : ce n'est pas une demande. Les guides, le contexte des hooks et le
/// dossier ultracode suivent le texte de l'utilisateur, ils ne l'ouvrent pas.
fn written_by_claw(text: &str) -> bool {
    text.starts_with(crate::harness::CLAW_NOTE_PREFIX)
        || text.starts_with(crate::conversation::STOP_HOOK_PREFIX)
}

/// Les demandes de l'utilisateur dans ces messages, mot pour mot : le premier
/// texte de chacun de ses messages.
fn user_requests(messages: &[ConversationMessage]) -> Vec<String> {
    messages
        .iter()
        .filter(|message| message.role == MessageRole::User)
        .filter_map(|message| match message.blocks.first() {
            Some(ContentBlock::Text { text }) => Some(text.trim()),
            _ => None,
        })
        .filter(|text| !text.is_empty() && !written_by_claw(text))
        .map(str::to_string)
        .collect()
}

/// Les demandes qui tiennent dans la place prévue : la première (la demande
/// d'origine), puis les plus récentes, chacune une fois à sa dernière place
/// (une demande renvoyée est la plus récente) ; celles du milieu sont
/// signalées comme omises.
fn kept_requests(requests: Vec<String>) -> Vec<String> {
    let Some((first, rest)) = requests.split_first() else {
        return requests;
    };
    let mut unique: Vec<&String> = Vec::new();
    for request in rest {
        unique.retain(|known| *known != request);
        unique.push(request);
    }
    let first = clip_request(first, FACTS_FIRST_REQUEST_LIMIT);
    let mut room = FACTS_REQUESTS_LIMIT.saturating_sub(first.chars().count());
    let mut recent = Vec::new();
    for request in unique.iter().rev() {
        let request = clip_request(request, FACTS_REQUEST_LIMIT);
        let size = request.chars().count();
        if size > room {
            break;
        }
        room -= size;
        recent.push(request);
    }
    let mut kept = vec![first];
    if recent.len() < unique.len() {
        kept.push(OMITTED_REQUESTS.to_string());
    }
    kept.extend(recent.into_iter().rev());
    kept
}

/// Une demande trop longue garde son début et sa fin : la consigne de
/// l'utilisateur y est presque toujours, autour de la sortie d'une commande
/// ou d'un fichier joint.
#[must_use]
pub fn clip_request(text: &str, limit: usize) -> String {
    let count = text.chars().count();
    if count <= limit {
        return text.to_string();
    }
    let head = limit / 3;
    let tail = limit - head;
    let start: String = text.chars().take(head).collect();
    let end: String = text.chars().skip(count - tail).collect();
    format!(
        "{start}\n[… {} characters omitted …]\n{end}",
        count - head - tail
    )
}

/// Les fichiers, chacun une fois, du moins récent au plus récent ; les plus
/// récents quand il y en a trop.
fn kept_files(files: Vec<String>) -> Vec<String> {
    let mut unique: Vec<String> = Vec::new();
    for file in files {
        unique.retain(|known| known != &file);
        unique.push(file);
    }
    let excess = unique.len().saturating_sub(FACTS_FILES_LIMIT);
    unique.split_off(excess)
}

/// Les faits tels qu'ils sont placés sous le résumé (vide s'il n'y a rien à
/// noter). Une demande sur plusieurs lignes garde ses lignes, en retrait.
fn render_facts(facts: &Facts) -> String {
    if facts.requests.is_empty() && facts.files.is_empty() {
        return String::new();
    }
    let mut lines = vec![FACTS_START.to_string()];
    if !facts.requests.is_empty() {
        lines.push(FACTS_REQUESTS.to_string());
        for request in &facts.requests {
            let mut parts = request.lines();
            lines.push(format!("- {}", parts.next().unwrap_or_default()));
            lines.extend(parts.map(|line| format!("  {line}")));
        }
    }
    if !facts.files.is_empty() {
        lines.push(FACTS_FILES.to_string());
        lines.extend(facts.files.iter().map(|file| format!("- {file}")));
    }
    lines.push(FACTS_END.to_string());
    lines.join("\n")
}

/// Les faits que Claw a placés sous un résumé précédent (son dernier bloc).
/// Ses marques sont des lignes entières : une ligne d'une demande citée
/// commence toujours par « - » ou par un retrait, elle ne peut pas les imiter.
fn recorded_facts(summary: &str) -> Facts {
    let mut facts = Facts::default();
    let lines: Vec<&str> = summary.lines().collect();
    let Some(start) = lines.iter().rposition(|line| *line == FACTS_START) else {
        return facts;
    };
    let mut in_files = false;
    for &line in &lines[start + 1..] {
        if line == FACTS_END {
            break;
        }
        if line == FACTS_REQUESTS || line == FACTS_FILES {
            in_files = line == FACTS_FILES;
        } else if let Some(item) = line.strip_prefix("- ") {
            let list = if in_files {
                &mut facts.files
            } else {
                &mut facts.requests
            };
            list.push(item.to_string());
        } else if !in_files {
            if let Some(request) = facts.requests.last_mut() {
                request.push('\n');
                request.push_str(line.strip_prefix("  ").unwrap_or(line));
            }
        }
    }
    facts
}

/// Le texte sans les faits de Claw (lignes entières de marque à marque) : un
/// modèle les recopie parfois dans son résumé, et ceux d'une compaction
/// précédente sont repris à part.
fn without_facts(text: &str) -> String {
    let mut kept = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        if inside {
            inside = line != FACTS_END;
        } else if line == FACTS_START {
            inside = true;
        } else {
            kept.push(line);
        }
    }
    kept.join("\n")
}

/// Le texte suivi des faits notés par Claw, hors des balises du résumé : ce
/// qu'une demande citée contient ne dérange pas sa mise en forme.
fn with_facts(text: &str, facts: &str) -> String {
    if facts.is_empty() {
        text.to_string()
    } else {
        format!("{}\n\n{facts}", text.trim_end())
    }
}

/// Les demandes de l'utilisateur que Claw a notées mot pour mot dans le
/// résumé qui ouvre une session compactée, les plus anciennes d'abord.
#[must_use]
pub fn recorded_requests(messages: &[ConversationMessage]) -> Vec<String> {
    messages
        .first()
        .and_then(extract_existing_compacted_summary)
        .map(|summary| recorded_facts(&summary).requests)
        .unwrap_or_default()
        .into_iter()
        .filter(|request| request != OMITTED_REQUESTS)
        .collect()
}

/// Recule le point de coupe pour que la partie conservée ne commence pas par un
/// résultat d'outil séparé de son appel (les API le refusent).
fn safe_keep_from(messages: &[ConversationMessage], mut keep_from: usize, minimum: usize) -> usize {
    while keep_from > minimum
        && messages
            .get(keep_from)
            .is_some_and(|message| message.role == MessageRole::Tool)
    {
        keep_from -= 1;
    }
    keep_from
}

/// Transcription lisible des messages à résumer, bornée en taille : le début
/// (demandes initiales) et la fin (travail en cours) sont gardés en priorité.
#[must_use]
pub fn render_transcript_for_summary(messages: &[ConversationMessage]) -> String {
    let mut transcript = String::new();
    for message in messages {
        let role = match message.role {
            MessageRole::System => "Earlier summary",
            MessageRole::User => "User",
            MessageRole::Assistant => "Assistant",
            MessageRole::Tool => "Tool",
        };
        for block in &message.blocks {
            let entry = match block {
                ContentBlock::Text { text } => {
                    format!("{role}: {}", truncate_summary(text, TRANSCRIPT_TEXT_LIMIT))
                }
                ContentBlock::ToolUse { name, input, .. } => format!(
                    "{role} called tool {name}: {}",
                    truncate_summary(input, TRANSCRIPT_TOOL_INPUT_LIMIT)
                ),
                ContentBlock::ToolResult {
                    tool_name,
                    output,
                    is_error,
                    ..
                } => format!(
                    "Result of {tool_name}{}: {}",
                    if *is_error { " (error)" } else { "" },
                    truncate_summary(output, TRANSCRIPT_TOOL_OUTPUT_LIMIT)
                ),
                ContentBlock::Thinking { .. } => continue,
                ContentBlock::Image { media_type, .. } => {
                    format!("{role} attached an image ({media_type})")
                }
            };
            transcript.push_str(&entry);
            transcript.push_str("\n\n");
        }
    }
    if transcript.len() <= TRANSCRIPT_TOTAL_LIMIT {
        return format!("{transcript}{SUMMARY_END_REMINDER}");
    }
    let head_end = floor_char_boundary(&transcript, TRANSCRIPT_HEAD_KEPT);
    let tail_start = floor_char_boundary(
        &transcript,
        transcript.len() - (TRANSCRIPT_TOTAL_LIMIT - TRANSCRIPT_HEAD_KEPT),
    );
    format!(
        "{}\n\n[… middle of the conversation omitted …]\n\n{}{SUMMARY_END_REMINDER}",
        &transcript[..head_end],
        &transcript[tail_start..]
    )
}

fn floor_char_boundary(text: &str, mut index: usize) -> usize {
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn compacted_summary_prefix_len(session: &Session) -> usize {
    usize::from(
        session
            .messages
            .first()
            .and_then(extract_existing_compacted_summary)
            .is_some(),
    )
}

fn summarize_messages(messages: &[ConversationMessage]) -> String {
    let user_messages = messages
        .iter()
        .filter(|message| message.role == MessageRole::User)
        .count();
    let assistant_messages = messages
        .iter()
        .filter(|message| message.role == MessageRole::Assistant)
        .count();
    let tool_messages = messages
        .iter()
        .filter(|message| message.role == MessageRole::Tool)
        .count();

    let mut tool_names = messages
        .iter()
        .flat_map(|message| message.blocks.iter())
        .filter_map(|block| match block {
            ContentBlock::ToolUse { name, .. } => Some(name.as_str()),
            ContentBlock::ToolResult { tool_name, .. } => Some(tool_name.as_str()),
            ContentBlock::Text { .. }
            | ContentBlock::Image { .. }
            | ContentBlock::Thinking { .. } => None,
        })
        .collect::<Vec<_>>();
    tool_names.sort_unstable();
    tool_names.dedup();

    let mut lines = vec![
        "<summary>".to_string(),
        "Conversation summary:".to_string(),
        format!(
            "- Scope: {} earlier messages compacted (user={}, assistant={}, tool={}).",
            messages.len(),
            user_messages,
            assistant_messages,
            tool_messages
        ),
    ];

    if !tool_names.is_empty() {
        lines.push(format!("- Tools mentioned: {}.", tool_names.join(", ")));
    }

    let recent_user_requests = collect_recent_role_summaries(messages, MessageRole::User, 3);
    if !recent_user_requests.is_empty() {
        lines.push("- Recent user requests:".to_string());
        lines.extend(
            recent_user_requests
                .into_iter()
                .map(|request| format!("  - {request}")),
        );
    }

    let pending_work = infer_pending_work(messages);
    if !pending_work.is_empty() {
        lines.push("- Pending work:".to_string());
        lines.extend(pending_work.into_iter().map(|item| format!("  - {item}")));
    }

    let key_files = collect_key_files(messages);
    if !key_files.is_empty() {
        lines.push(format!("- Key files referenced: {}.", key_files.join(", ")));
    }

    if let Some(current_work) = infer_current_work(messages) {
        lines.push(format!("- Current work: {current_work}"));
    }

    lines.push("- Key timeline:".to_string());
    for message in messages {
        let role = match message.role {
            MessageRole::System => "system",
            MessageRole::User => "user",
            MessageRole::Assistant => "assistant",
            MessageRole::Tool => "tool",
        };
        let content = message
            .blocks
            .iter()
            .map(summarize_block)
            .collect::<Vec<_>>()
            .join(" | ");
        lines.push(format!("  - {role}: {content}"));
    }
    lines.push("</summary>".to_string());
    lines.join("\n")
}

fn merge_compact_summaries(existing_summary: Option<&str>, new_summary: &str) -> String {
    let Some(existing_summary) = existing_summary else {
        return new_summary.to_string();
    };

    let previous_highlights = extract_summary_highlights(existing_summary);
    let new_formatted_summary = format_compact_summary(new_summary);
    let new_highlights = extract_summary_highlights(&new_formatted_summary);
    let new_timeline = extract_summary_timeline(&new_formatted_summary);

    let mut lines = vec!["<summary>".to_string(), "Conversation summary:".to_string()];

    if !previous_highlights.is_empty() {
        lines.push("- Previously compacted context:".to_string());
        lines.extend(
            previous_highlights
                .into_iter()
                .map(|line| format!("  {line}")),
        );
    }

    if !new_highlights.is_empty() {
        lines.push("- Newly compacted context:".to_string());
        lines.extend(new_highlights.into_iter().map(|line| format!("  {line}")));
    }

    if !new_timeline.is_empty() {
        lines.push("- Key timeline:".to_string());
        lines.extend(new_timeline.into_iter().map(|line| format!("  {line}")));
    }

    lines.push("</summary>".to_string());
    lines.join("\n")
}

fn summarize_block(block: &ContentBlock) -> String {
    let raw = match block {
        ContentBlock::Text { text } => text.clone(),
        ContentBlock::ToolUse { name, input, .. } => format!("tool_use {name}({input})"),
        ContentBlock::ToolResult {
            tool_name,
            output,
            is_error,
            ..
        } => format!(
            "tool_result {tool_name}: {}{output}",
            if *is_error { "error " } else { "" }
        ),
        ContentBlock::Image { media_type, .. } => format!("image ({media_type})"),
        ContentBlock::Thinking { thinking, .. } => format!("thinking: {thinking}"),
    };
    truncate_summary(&raw, 160)
}

fn collect_recent_role_summaries(
    messages: &[ConversationMessage],
    role: MessageRole,
    limit: usize,
) -> Vec<String> {
    messages
        .iter()
        .filter(|message| message.role == role)
        .rev()
        .filter_map(|message| first_text_block(message))
        .take(limit)
        .map(|text| truncate_summary(text, 160))
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

fn infer_pending_work(messages: &[ConversationMessage]) -> Vec<String> {
    messages
        .iter()
        .rev()
        .filter_map(first_text_block)
        .filter(|text| {
            let lowered = text.to_ascii_lowercase();
            lowered.contains("todo")
                || lowered.contains("next")
                || lowered.contains("pending")
                || lowered.contains("follow up")
                || lowered.contains("remaining")
        })
        .take(3)
        .map(|text| truncate_summary(text, 160))
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

fn collect_key_files(messages: &[ConversationMessage]) -> Vec<String> {
    let mut files = messages
        .iter()
        .flat_map(|message| message.blocks.iter())
        .map(|block| match block {
            ContentBlock::Text { text } => text.as_str(),
            ContentBlock::ToolUse { input, .. } => input.as_str(),
            ContentBlock::ToolResult { output, .. } => output.as_str(),
            ContentBlock::Image { .. } | ContentBlock::Thinking { .. } => "",
        })
        .flat_map(extract_file_candidates)
        .collect::<Vec<_>>();
    files.sort();
    files.dedup();
    files.into_iter().take(8).collect()
}

fn infer_current_work(messages: &[ConversationMessage]) -> Option<String> {
    messages
        .iter()
        .rev()
        .filter_map(first_text_block)
        .find(|text| !text.trim().is_empty())
        .map(|text| truncate_summary(text, 200))
}

fn first_text_block(message: &ConversationMessage) -> Option<&str> {
    message.blocks.iter().find_map(|block| match block {
        ContentBlock::Text { text } if !text.trim().is_empty() => Some(text.as_str()),
        ContentBlock::ToolUse { .. }
        | ContentBlock::ToolResult { .. }
        | ContentBlock::Image { .. }
        | ContentBlock::Thinking { .. }
        | ContentBlock::Text { .. } => None,
    })
}

fn has_interesting_extension(candidate: &str) -> bool {
    std::path::Path::new(candidate)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            ["rs", "ts", "tsx", "js", "json", "md"]
                .iter()
                .any(|expected| extension.eq_ignore_ascii_case(expected))
        })
}

fn extract_file_candidates(content: &str) -> Vec<String> {
    content
        .split_whitespace()
        .filter_map(|token| {
            let candidate = token.trim_matches(|char: char| {
                matches!(char, ',' | '.' | ':' | ';' | ')' | '(' | '"' | '\'' | '`')
            });
            if candidate.contains('/') && has_interesting_extension(candidate) {
                Some(candidate.to_string())
            } else {
                None
            }
        })
        .collect()
}

fn truncate_summary(content: &str, max_chars: usize) -> String {
    if content.chars().count() <= max_chars {
        return content.to_string();
    }
    let mut truncated = content.chars().take(max_chars).collect::<String>();
    truncated.push('…');
    truncated
}

pub(crate) fn estimate_message_tokens(message: &ConversationMessage) -> usize {
    message
        .blocks
        .iter()
        .map(|block| match block {
            ContentBlock::Text { text } => text.len() / 4 + 1,
            ContentBlock::ToolUse { name, input, .. } => (name.len() + input.len()) / 4 + 1,
            ContentBlock::ToolResult {
                tool_name, output, ..
            } => (tool_name.len() + output.len()) / 4 + 1,
            // Une image coûte environ 1 500 jetons, quelle que soit sa taille en base64.
            ContentBlock::Image { .. } => 1_500,
            ContentBlock::Thinking { thinking, .. } => thinking.len() / 4 + 1,
        })
        .sum()
}

fn extract_tag_block(content: &str, tag: &str) -> Option<String> {
    let start = format!("<{tag}>");
    let end = format!("</{tag}>");
    let start_index = content.find(&start)? + start.len();
    let end_index = content[start_index..].find(&end)? + start_index;
    Some(content[start_index..end_index].to_string())
}

fn strip_tag_block(content: &str, tag: &str) -> String {
    let start = format!("<{tag}>");
    let end = format!("</{tag}>");
    if let (Some(start_index), Some(end_index_rel)) = (content.find(&start), content.find(&end)) {
        let end_index = end_index_rel + end.len();
        let mut stripped = String::new();
        stripped.push_str(&content[..start_index]);
        stripped.push_str(&content[end_index..]);
        stripped
    } else {
        content.to_string()
    }
}

fn collapse_blank_lines(content: &str) -> String {
    let mut result = String::new();
    let mut last_blank = false;
    for line in content.lines() {
        let is_blank = line.trim().is_empty();
        if is_blank && last_blank {
            continue;
        }
        result.push_str(line);
        result.push('\n');
        last_blank = is_blank;
    }
    result
}

fn extract_existing_compacted_summary(message: &ConversationMessage) -> Option<String> {
    if message.role != MessageRole::System {
        return None;
    }

    let text = first_text_block(message)?;
    let summary = text.strip_prefix(COMPACT_CONTINUATION_PREAMBLE)?;
    // Ces consignes terminent le message : une demande citée plus haut qui
    // les contiendrait ne coupe pas le résumé.
    let summary = summary
        .rsplit_once(&format!("\n\n{COMPACT_RECENT_MESSAGES_NOTE}"))
        .map_or(summary, |(value, _)| value);
    let summary = summary
        .rsplit_once(&format!("\n{COMPACT_DIRECT_RESUME_INSTRUCTION}"))
        .map_or(summary, |(value, _)| value);
    Some(summary.trim().to_string())
}

fn extract_summary_highlights(summary: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut in_timeline = false;

    for line in format_compact_summary(summary).lines() {
        let trimmed = line.trim_end();
        if trimmed.is_empty() || trimmed == "Summary:" || trimmed == "Conversation summary:" {
            continue;
        }
        if trimmed == "- Key timeline:" {
            in_timeline = true;
            continue;
        }
        if in_timeline {
            continue;
        }
        lines.push(trimmed.to_string());
    }

    lines
}

fn extract_summary_timeline(summary: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut in_timeline = false;

    for line in format_compact_summary(summary).lines() {
        let trimmed = line.trim_end();
        if trimmed == "- Key timeline:" {
            in_timeline = true;
            continue;
        }
        if !in_timeline {
            continue;
        }
        if trimmed.is_empty() {
            break;
        }
        lines.push(trimmed.to_string());
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::{
        collect_key_files, compact_session, estimate_session_tokens, format_compact_summary,
        get_compact_continuation_message, infer_pending_work, should_compact, CompactionConfig,
    };
    use crate::session::{ContentBlock, ConversationMessage, MessageRole, Session};

    #[test]
    fn a_summary_that_is_not_one_is_replaced_and_claw_adds_the_facts() {
        use super::{compact_session_with, render_transcript_for_summary, summary_is_usable};
        let session = Session {
            version: 1,
            messages: vec![
                ConversationMessage::user_text(
                    "Traduis les mangas du japonais vers le français,\nen trois étapes",
                ),
                ConversationMessage::assistant(vec![ContentBlock::ToolUse {
                    id: "t1".to_string(),
                    name: "write_file".to_string(),
                    input: r#"{"path":"backend/app/main.py","content":"x"}"#.to_string(),
                }]),
                ConversationMessage::tool_result("t1", "write_file", "ok", false),
                ConversationMessage::user_text("[Claw] note de Claw, à ne pas garder"),
                ConversationMessage::assistant(vec![ContentBlock::Text {
                    text: "recent ".repeat(300),
                }]),
                ConversationMessage::user_text("et maintenant ?"),
            ],
        };
        let config = CompactionConfig {
            preserve_recent_messages: 2,
            max_estimated_tokens: 1,
        };
        // La conversation à résumer finit par la consigne de résumer.
        let transcript = render_transcript_for_summary(&session.messages);
        assert!(transcript.ends_with("5. Open problems 6. Next step.]"));
        // Le modèle a continué la conversation (session réelle) : son texte
        // est écarté, le résumé de Claw le remplace.
        let reply = "L'installation est en cours. Une fois terminée :\n```powershell\n# Dans le même terminal backend\nuvicorn app.main:app --reload\n```\n**Résultat :**\n- Frontend: http://localhost:3000";
        assert!(!summary_is_usable(reply, &session.messages));
        let result = compact_session_with(&session, config, |_| Some(reply.to_string()));
        assert!(
            !result.summary.contains("installation est en cours"),
            "{}",
            result.summary
        );
        assert!(result.summary.contains("Conversation summary"));
        assert!(!result.model_summary);
        // Les faits suivent le résumé, hors de ses balises.
        assert!(
            result.summary.ends_with(
                "</summary>\n\n[Recorded by Claw, exact]\nUser requests, word for word, oldest first:\n- Traduis les mangas du japonais vers le français,\n  en trois étapes\nFiles written or edited:\n- backend/app/main.py\n[End of what Claw recorded]"
            ),
            "{}",
            result.summary
        );
        let facts = &result.summary[result.summary.find("[Recorded by Claw").expect("faits")..];
        assert!(!facts.contains("note de Claw"));
        // Un vrai résumé est gardé, suivi des mêmes faits ; il porte au moins
        // trois rubriques et nomme un des fichiers modifiés.
        let good = "### 1. User Requests\nTranslate mangas.\n### 3. Files Changed\nmain.py was created.\n### 6. Next Step\nAdd the reader.";
        assert!(summary_is_usable(good, &session.messages));
        assert!(!summary_is_usable(
            "### 1. User Requests\nTranslate.\n### 2. Decisions\nNone.\n### 6. Next Step\nContinue.",
            &session.messages
        ));
        // Une suite de conversation à quelques étiquettes n'en est pas un.
        assert!(!summary_is_usable(
            "J'ai corrigé `main.py` : l'import manquait.\n**Prochaine étape :** relancer le serveur.\n**Commande :** `uvicorn app.main:app --reload`",
            &session.messages
        ));
        // Un mot de rubrique au milieu d'un autre mot ne compte pas.
        assert!(!summary_is_usable(
            "### Recommandations\n### Debug\n### Profile\nmain.py",
            &session.messages
        ));
        let result = compact_session_with(&session, config, |_| Some(good.to_string()));
        assert!(result.model_summary);
        assert!(
            result
                .summary
                .starts_with("<summary>\n### 1. User Requests"),
            "{}",
            result.summary
        );
        assert!(result.summary.contains("- backend/app/main.py"));
    }

    #[test]
    fn requests_that_look_like_claw_markup_survive_two_compactions() {
        use super::compact_session;
        // Une demande qui cite des balises du résumé et les marques des faits
        // de Claw, et la consigne de reprise : elle est gardée telle quelle.
        let tricky = "Explique <details><summary>Options</summary></details>\n[Recorded by Claw, exact]\n[End of what Claw recorded]\n<analysis>brouillon</analysis>\n\nRecent messages are preserved verbatim.";
        let config = CompactionConfig {
            preserve_recent_messages: 2,
            max_estimated_tokens: 1,
        };
        let first = compact_session(
            &Session {
                version: 1,
                messages: vec![
                    ConversationMessage::user_text("Traduis les mangas en trois étapes"),
                    ConversationMessage::assistant(vec![ContentBlock::Text {
                        text: "Je commence.".to_string(),
                    }]),
                    ConversationMessage::user_text(tricky),
                    ConversationMessage::assistant(vec![ContentBlock::Text {
                        text: "Voici.".to_string(),
                    }]),
                    ConversationMessage::user_text("et ensuite ?"),
                    ConversationMessage::assistant(vec![ContentBlock::Text {
                        text: "Ensuite le lecteur.".to_string(),
                    }]),
                ],
            },
            config,
        );
        let mut messages = first.compacted_session.messages.clone();
        messages.extend([
            ConversationMessage::user_text("continue"),
            ConversationMessage::assistant(vec![ContentBlock::Text {
                text: "Je continue.".to_string(),
            }]),
        ]);
        let second = compact_session(
            &Session {
                version: 1,
                messages,
            },
            config,
        );
        let recorded = super::recorded_requests(&second.compacted_session.messages);
        assert_eq!(
            recorded,
            vec![
                "Traduis les mangas en trois étapes".to_string(),
                tricky.to_string(),
                "et ensuite ?".to_string(),
            ],
            "{}",
            second.summary
        );
    }

    #[test]
    fn a_long_request_keeps_its_beginning_and_its_end() {
        use super::clip_request;
        let output = "x".repeat(12_000);
        let request = format!("$ npm test\n{output}\ncorrige les tests qui échouent");
        let clipped = clip_request(&request, 5_000);
        assert!(clipped.starts_with("$ npm test"));
        assert!(clipped.ends_with("corrige les tests qui échouent"));
        assert!(clipped.contains("characters omitted"));
        assert!(clipped.chars().count() < 5_100);
        assert_eq!(clip_request("court", 5_000), "court");
    }

    #[test]
    fn claw_keeps_the_first_request_and_the_latest_when_space_runs_out() {
        use super::{kept_requests, OMITTED_REQUESTS};
        let requests: Vec<String> = (0..40)
            .map(|index| format!("demande {index} {}", "x".repeat(400)))
            .collect();
        let kept = kept_requests(requests.clone());
        assert_eq!(kept[0], requests[0]);
        assert_eq!(kept[1], OMITTED_REQUESTS);
        assert_eq!(kept.last(), requests.last());
        let size: usize = kept.iter().map(|request| request.chars().count()).sum();
        assert!(size <= 8_100, "{size}");
        // Une demande répétée n'est notée qu'une fois, à sa dernière place :
        // c'est la plus récente.
        let kept = kept_requests(vec![
            "fais X".to_string(),
            "continue".to_string(),
            "fais Y".to_string(),
            "continue".to_string(),
        ]);
        assert_eq!(
            kept,
            vec![
                "fais X".to_string(),
                "fais Y".to_string(),
                "continue".to_string()
            ]
        );
    }

    #[test]
    fn formats_compact_summary_like_upstream() {
        let summary = "<analysis>scratch</analysis>\n<summary>Kept work</summary>";
        assert_eq!(format_compact_summary(summary), "Summary:\nKept work");
    }

    #[test]
    fn leaves_small_sessions_unchanged() {
        let session = Session {
            version: 1,
            messages: vec![ConversationMessage::user_text("hello")],
        };

        let result = compact_session(&session, CompactionConfig::default());
        assert_eq!(result.removed_message_count, 0);
        assert_eq!(result.compacted_session, session);
        assert!(result.summary.is_empty());
        assert!(result.formatted_summary.is_empty());
    }

    #[test]
    fn compacts_older_messages_into_a_system_summary() {
        let session = Session {
            version: 1,
            messages: vec![
                ConversationMessage::user_text("one ".repeat(2_000)),
                ConversationMessage::assistant(vec![ContentBlock::Text {
                    text: "two ".repeat(200),
                }]),
                ConversationMessage::tool_result("1", "bash", "ok ".repeat(200), false),
                ConversationMessage {
                    role: MessageRole::Assistant,
                    blocks: vec![ContentBlock::Text {
                        text: "recent".to_string(),
                    }],
                    usage: None,
                },
            ],
        };

        let result = compact_session(
            &session,
            CompactionConfig {
                preserve_recent_messages: 2,
                max_estimated_tokens: 1,
            },
        );

        // La coupe recule d'un message : la partie conservée ne doit pas commencer
        // par un résultat d'outil séparé de son appel.
        assert_eq!(result.removed_message_count, 1);
        assert_eq!(
            result.compacted_session.messages[0].role,
            MessageRole::System
        );
        assert_ne!(result.compacted_session.messages[1].role, MessageRole::Tool);
        assert!(matches!(
            &result.compacted_session.messages[0].blocks[0],
            ContentBlock::Text { text } if text.contains("Summary:")
        ));
        assert!(result.formatted_summary.contains("Scope:"));
        assert!(result.formatted_summary.contains("Key timeline:"));
        assert!(should_compact(
            &session,
            CompactionConfig {
                preserve_recent_messages: 2,
                max_estimated_tokens: 1,
            }
        ));
        assert!(
            estimate_session_tokens(&result.compacted_session) < estimate_session_tokens(&session)
        );
    }

    #[test]
    fn keeps_previous_compacted_context_when_compacting_again() {
        let initial_session = Session {
            version: 1,
            messages: vec![
                ConversationMessage::user_text("Investigate rust/crates/runtime/src/compact.rs"),
                ConversationMessage::assistant(vec![ContentBlock::Text {
                    text: "I will inspect the compact flow.".to_string(),
                }]),
                ConversationMessage::user_text(
                    "Also update rust/crates/runtime/src/conversation.rs",
                ),
                ConversationMessage::assistant(vec![ContentBlock::Text {
                    text: "Next: preserve prior summary context during auto compact.".to_string(),
                }]),
            ],
        };
        let config = CompactionConfig {
            preserve_recent_messages: 2,
            max_estimated_tokens: 1,
        };

        let first = compact_session(&initial_session, config);
        let mut follow_up_messages = first.compacted_session.messages.clone();
        follow_up_messages.extend([
            ConversationMessage::user_text("Please add regression tests for compaction."),
            ConversationMessage::assistant(vec![ContentBlock::Text {
                text: "Working on regression coverage now.".to_string(),
            }]),
        ]);

        let second = compact_session(
            &Session {
                version: 1,
                messages: follow_up_messages,
            },
            config,
        );

        assert!(second
            .formatted_summary
            .contains("Previously compacted context:"));
        assert!(second
            .formatted_summary
            .contains("Scope: 2 earlier messages compacted"));
        assert!(second
            .formatted_summary
            .contains("Newly compacted context:"));
        assert!(second
            .formatted_summary
            .contains("Also update rust/crates/runtime/src/conversation.rs"));
        assert!(matches!(
            &second.compacted_session.messages[0].blocks[0],
            ContentBlock::Text { text }
                if text.contains("Previously compacted context:")
                    && text.contains("Newly compacted context:")
        ));
        assert!(matches!(
            &second.compacted_session.messages[1].blocks[0],
            ContentBlock::Text { text } if text.contains("Please add regression tests for compaction.")
        ));
        // Les demandes de la première compaction restent mot pour mot dans
        // les faits de Claw, avant celles de la seconde, une seule fois.
        assert!(
            second.summary.contains(
                "- Investigate rust/crates/runtime/src/compact.rs\n- Also update rust/crates/runtime/src/conversation.rs"
            ),
            "{}",
            second.summary
        );
        assert_eq!(
            second.summary.matches("[Recorded by Claw, exact]").count(),
            1
        );
    }

    #[test]
    fn ignores_existing_compacted_summary_when_deciding_to_recompact() {
        let summary = "<summary>Conversation summary:\n- Scope: earlier work preserved.\n- Key timeline:\n  - user: large preserved context\n</summary>";
        let session = Session {
            version: 1,
            messages: vec![
                ConversationMessage {
                    role: MessageRole::System,
                    blocks: vec![ContentBlock::Text {
                        text: get_compact_continuation_message(summary, true, true),
                    }],
                    usage: None,
                },
                ConversationMessage::user_text("tiny"),
                ConversationMessage::assistant(vec![ContentBlock::Text {
                    text: "recent".to_string(),
                }]),
            ],
        };

        assert!(!should_compact(
            &session,
            CompactionConfig {
                preserve_recent_messages: 2,
                max_estimated_tokens: 1,
            }
        ));
    }

    #[test]
    fn truncates_long_blocks_in_summary() {
        let summary = super::summarize_block(&ContentBlock::Text {
            text: "x".repeat(400),
        });
        assert!(summary.ends_with('…'));
        assert!(summary.chars().count() <= 161);
    }

    #[test]
    fn extracts_key_files_from_message_content() {
        let files = collect_key_files(&[ConversationMessage::user_text(
            "Update rust/crates/runtime/src/compact.rs and rust/crates/tools/src/lib.rs next.",
        )]);
        assert!(files.contains(&"rust/crates/runtime/src/compact.rs".to_string()));
        assert!(files.contains(&"rust/crates/tools/src/lib.rs".to_string()));
    }

    #[test]
    fn infers_pending_work_from_recent_messages() {
        let pending = infer_pending_work(&[
            ConversationMessage::user_text("done"),
            ConversationMessage::assistant(vec![ContentBlock::Text {
                text: "Next: update tests and follow up on remaining CLI polish.".to_string(),
            }]),
        ]);
        assert_eq!(pending.len(), 1);
        assert!(pending[0].contains("Next: update tests"));
    }
}
