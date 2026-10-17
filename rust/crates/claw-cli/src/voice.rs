//! Dictée vocale (comme `/voice` de Claude Code) : dans la saisie, maintenir
//! Espace enregistre (mode `hold`), ou une frappe démarre et une autre
//! arrête (mode `tap`) ; le texte transcrit s'insère au curseur.
//!
//! Version gratuite : la transcription passe par le premier moteur
//! disponible, dans l'ordre :
//! - une commande à vous (`CLAW_VOICE_COMMAND`) ;
//! - whisper.cpp en local, hors ligne (`whisper-cli` et `CLAW_WHISPER_MODEL`) ;
//! - Groq (`GROQ_API_KEY`, palier gratuit, Whisper) ;
//! - sous Windows, la reconnaissance vocale du système, hors ligne et sans clé ;
//! - un modèle audio d'`OpenRouter` choisi avec `CLAW_VOICE_MODEL` (les modèles
//!   gratuits n'y transcrivent pas aujourd'hui) ;
//! - `OpenAI` (`OPENAI_API_KEY`, payant), seulement si demandé.
//!
//! `CLAW_VOICE_ENGINE` en impose un. L'enregistrement passe par `cpal`
//! (Windows, macOS) ou `arecord`, `parecord`, `ffmpeg` (Linux) ; rien n'est
//! enregistré hors d'une dictée, et le son n'est gardé que le temps de la
//! transcription.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

/// Durée d'une dictée au plus.
pub(crate) const MAX_RECORDING: Duration = Duration::from_secs(120);
/// Fréquence attendue par Whisper.
const TARGET_RATE: u32 = 16_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    /// Maintenir Espace pendant qu'on parle.
    Hold,
    /// Une frappe démarre, la suivante arrête.
    Tap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Settings {
    pub mode: Mode,
    /// Mode `hold` : la demande part dès la touche relâchée.
    pub auto_submit: bool,
}

fn config() -> Option<runtime::RuntimeConfig> {
    std::env::current_dir()
        .ok()
        .and_then(|cwd| runtime::ConfigLoader::default_for(cwd).load().ok())
}

/// Dictée active : réglage `voice` (`enabled`, `mode`, `autoSubmit`), ou
/// l'ancien `voiceEnabled` ; `CLAW_VOICE=0` la coupe.
pub(crate) fn settings() -> Option<Settings> {
    if std::env::var("CLAW_VOICE").is_ok_and(|value| value.trim() == "0") {
        return None;
    }
    let config = config()?;
    let voice = config.get("voice").and_then(runtime::JsonValue::as_object);
    let enabled = voice
        .and_then(|voice| voice.get("enabled"))
        .and_then(runtime::JsonValue::as_bool)
        .or_else(|| {
            config
                .get("voiceEnabled")
                .and_then(runtime::JsonValue::as_bool)
        })
        .unwrap_or(false);
    if !enabled {
        return None;
    }
    let mode = match voice
        .and_then(|voice| voice.get("mode"))
        .and_then(runtime::JsonValue::as_str)
    {
        Some("tap") => Mode::Tap,
        _ => Mode::Hold,
    };
    let auto_submit = voice
        .and_then(|voice| voice.get("autoSubmit"))
        .and_then(runtime::JsonValue::as_bool)
        .unwrap_or(false);
    Some(Settings { mode, auto_submit })
}

/// Langue de la dictée (code ISO 639-1) : réglage `language` (« french »,
/// « fr »…), sinon la langue du système.
fn language() -> Option<String> {
    let setting = config().and_then(|config| {
        config
            .get("language")
            .and_then(runtime::JsonValue::as_str)
            .map(str::to_lowercase)
    });
    let from_system = || {
        ["LC_ALL", "LANG", "LANGUAGE"]
            .iter()
            .find_map(|name| std::env::var(name).ok())
            .map(|value| value.to_lowercase())
    };
    let raw = setting.or_else(from_system)?;
    let code = match raw.trim() {
        "french" | "français" | "francais" => "fr",
        "english" | "anglais" => "en",
        "spanish" | "español" | "espagnol" => "es",
        "german" | "deutsch" | "allemand" => "de",
        "italian" | "italiano" | "italien" => "it",
        "portuguese" | "português" | "portugais" => "pt",
        "dutch" | "nederlands" | "néerlandais" => "nl",
        "japanese" | "japonais" => "ja",
        "chinese" | "chinois" => "zh",
        "arabic" | "arabe" => "ar",
        "russian" | "russe" => "ru",
        other => {
            let code: String = other
                .chars()
                .take_while(char::is_ascii_alphabetic)
                .collect();
            return (code.len() == 2).then_some(code);
        }
    };
    Some(code.to_string())
}

// --- Enregistrement -------------------------------------------------------------

/// Enregistrement en cours (le son ne quitte pas la mémoire, sauf pour la
/// transcription).
pub(crate) struct Recorder {
    started: Instant,
    source: Source,
}

/// D'où vient le son : le micro, ou un fichier WAV (`CLAW_VOICE_INPUT_FILE`,
/// pour essayer la dictée sans micro).
enum Source {
    Device(Capture),
    File(PathBuf),
}

/// Début d'une dictée.
pub(crate) fn start() -> Result<Recorder, String> {
    let source = match env("CLAW_VOICE_INPUT_FILE") {
        Some(file) => Source::File(PathBuf::from(file)),
        None => Source::Device(start_device()?),
    };
    Ok(Recorder {
        started: Instant::now(),
        source,
    })
}

#[cfg(any(windows, target_os = "macos"))]
struct Capture {
    stream: cpal::Stream,
    samples: std::sync::Arc<std::sync::Mutex<Vec<f32>>>,
    rate: u32,
}

#[cfg(not(any(windows, target_os = "macos")))]
struct Capture {
    child: std::process::Child,
    file: PathBuf,
}

impl Recorder {
    #[must_use]
    pub(crate) fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// Abandon : rien n'est gardé.
    pub(crate) fn cancel(self) {
        let _ = self.finish();
    }

    /// Fin de la dictée : le son en WAV.
    pub(crate) fn finish(self) -> Result<Vec<u8>, String> {
        match self.source {
            Source::Device(capture) => capture.finish(),
            Source::File(file) => {
                std::fs::read(&file).map_err(|error| format!("{} : {error}", file.display()))
            }
        }
    }
}

/// Micro par défaut (pour `/voice`), sans rien enregistrer.
#[cfg(any(windows, target_os = "macos"))]
pub(crate) fn input_device() -> Option<String> {
    use cpal::traits::{DeviceTrait, HostTrait};
    cpal::default_host()
        .default_input_device()
        .and_then(|device| device.name().ok())
}

#[cfg(any(windows, target_os = "macos"))]
fn start_device() -> Result<Capture, String> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    let device = cpal::default_host()
        .default_input_device()
        .ok_or("aucun micro trouvé")?;
    let config = device
        .default_input_config()
        .map_err(|error| format!("micro indisponible : {error}"))?;
    let rate = config.sample_rate().0;
    let channels = usize::from(config.channels().max(1));
    let samples = std::sync::Arc::new(std::sync::Mutex::new(Vec::<f32>::new()));
    let sink = std::sync::Arc::clone(&samples);
    // Mono : moyenne des canaux de chaque trame.
    let push = move |frames: &mut dyn Iterator<Item = f32>| {
        let mut buffer = sink
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let all: Vec<f32> = frames.collect();
        for frame in all.chunks(channels) {
            #[allow(clippy::cast_precision_loss)]
            buffer.push(frame.iter().sum::<f32>() / frame.len() as f32);
        }
    };
    let error = |error: cpal::StreamError| eprintln!("[dictée] {error}");
    let stream_config: cpal::StreamConfig = config.clone().into();
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => device.build_input_stream(
            &stream_config,
            move |data: &[f32], _: &cpal::InputCallbackInfo| push(&mut data.iter().copied()),
            error,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_input_stream(
            &stream_config,
            move |data: &[i16], _: &cpal::InputCallbackInfo| {
                push(&mut data.iter().map(|sample| f32::from(*sample) / 32_768.0));
            },
            error,
            None,
        ),
        cpal::SampleFormat::U16 => device.build_input_stream(
            &stream_config,
            move |data: &[u16], _: &cpal::InputCallbackInfo| {
                push(
                    &mut data
                        .iter()
                        .map(|sample| (f32::from(*sample) - 32_768.0) / 32_768.0),
                );
            },
            error,
            None,
        ),
        other => return Err(format!("format du micro non pris en charge : {other}")),
    }
    .map_err(|error| format!("micro indisponible : {error}"))?;
    stream
        .play()
        .map_err(|error| format!("micro indisponible : {error}"))?;
    Ok(Capture {
        stream,
        samples,
        rate,
    })
}

#[cfg(any(windows, target_os = "macos"))]
impl Capture {
    /// Fin de l'enregistrement : le son en WAV mono 16 kHz.
    #[allow(clippy::unnecessary_wraps)]
    fn finish(self) -> Result<Vec<u8>, String> {
        drop(self.stream);
        let samples = std::mem::take(
            &mut *self
                .samples
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        Ok(wav(
            &resample(&samples, self.rate, TARGET_RATE),
            TARGET_RATE,
        ))
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
pub(crate) fn input_device() -> Option<String> {
    ["arecord", "parecord", "ffmpeg"]
        .iter()
        .find(|program| available(program))
        .map(|program| format!("micro par défaut ({program})"))
}

#[cfg(not(any(windows, target_os = "macos")))]
fn start_device() -> Result<Capture, String> {
    let file = std::env::temp_dir().join(format!("claw-voice-{}.wav", std::process::id()));
    let target = file.display().to_string();
    let rate = TARGET_RATE.to_string();
    let candidates: [(&str, Vec<String>); 3] = [
        (
            "arecord",
            vec![
                "-q", "-f", "S16_LE", "-r", &rate, "-c", "1", "-t", "wav", &target,
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
        ),
        (
            "parecord",
            vec![
                &format!("--rate={rate}"),
                "--channels=1",
                "--format=s16le",
                "--file-format=wav",
                &target,
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
        ),
        (
            "ffmpeg",
            vec![
                "-loglevel",
                "quiet",
                "-f",
                "pulse",
                "-i",
                "default",
                "-ac",
                "1",
                "-ar",
                &rate,
                "-y",
                &target,
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
        ),
    ];
    for (program, args) in candidates {
        if !available(program) {
            continue;
        }
        let child = Command::new(program)
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("{program} : {error}"))?;
        return Ok(Capture { child, file });
    }
    Err("aucun enregistreur trouvé : installez alsa-utils (arecord), pulseaudio-utils (parecord) ou ffmpeg".to_string())
}

#[cfg(not(any(windows, target_os = "macos")))]
impl Capture {
    fn finish(mut self) -> Result<Vec<u8>, String> {
        // SIGINT : l'enregistreur referme proprement son fichier.
        let _ = Command::new("kill")
            .args(["-INT", &self.child.id().to_string()])
            .status();
        let _ = self.child.wait();
        let audio = std::fs::read(&self.file).map_err(|error| error.to_string());
        let _ = std::fs::remove_file(&self.file);
        audio
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
fn available(program: &str) -> bool {
    Command::new(program)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

/// Rééchantillonnage linéaire.
fn resample(samples: &[f32], from: u32, to: u32) -> Vec<f32> {
    if from == to || samples.is_empty() || from == 0 {
        return samples.to_vec();
    }
    let ratio = f64::from(from) / f64::from(to);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let length = (samples.len() as f64 / ratio) as usize;
    (0..length)
        .map(|index| {
            #[allow(clippy::cast_precision_loss)]
            let position = index as f64 * ratio;
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let base = position as usize;
            let next = (base + 1).min(samples.len() - 1);
            #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
            let weight = (position - base as f64) as f32;
            samples[base] * (1.0 - weight) + samples[next] * weight
        })
        .collect()
}

/// WAV PCM 16 bits mono.
fn wav(samples: &[f32], rate: u32) -> Vec<u8> {
    let data_len = u32::try_from(samples.len() * 2).unwrap_or(u32::MAX);
    let mut out = Vec::with_capacity(44 + samples.len() * 2);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16_u32.to_le_bytes());
    out.extend_from_slice(&1_u16.to_le_bytes());
    out.extend_from_slice(&1_u16.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2_u16.to_le_bytes());
    out.extend_from_slice(&16_u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        #[allow(clippy::cast_possible_truncation)]
        let value = (sample.clamp(-1.0, 1.0) * 32_767.0) as i16;
        out.extend_from_slice(&value.to_le_bytes());
    }
    out
}

// --- Transcription --------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
enum Engine {
    Command(String),
    Whisper(String),
    Groq(String),
    /// Reconnaissance vocale de Windows (`System.Speech`), hors ligne.
    Windows,
    OpenRouter(String),
    OpenAi(String),
}

impl Engine {
    fn label(&self) -> String {
        match self {
            Self::Command(command) => format!("commande ({command})"),
            Self::Whisper(model) => format!("whisper.cpp local ({model})"),
            Self::Groq(_) => "Groq Whisper (palier gratuit)".to_string(),
            Self::Windows => "reconnaissance vocale de Windows (hors ligne ; GROQ_API_KEY ou whisper.cpp pour mieux)".to_string(),
            Self::OpenRouter(_) => format!("OpenRouter ({})", openrouter_model()),
            Self::OpenAi(_) => "OpenAI (payant)".to_string(),
        }
    }
}

fn env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn key(name: &str) -> Option<String> {
    env(name).or_else(|| api::key_helper::key(name))
}

/// Premier moteur disponible (ou celui de `CLAW_VOICE_ENGINE`).
fn engine() -> Result<Engine, String> {
    let wanted = env("CLAW_VOICE_ENGINE").map(|value| value.to_lowercase());
    let pick = |name: &str| wanted.as_deref().is_none_or(|wanted| wanted == name);
    if pick("command") {
        if let Some(command) = env("CLAW_VOICE_COMMAND") {
            return Ok(Engine::Command(command));
        }
    }
    if pick("whisper") {
        if let Some(model) = env("CLAW_WHISPER_MODEL") {
            return Ok(Engine::Whisper(model));
        }
    }
    if pick("groq") {
        if let Some(key) = key("GROQ_API_KEY") {
            return Ok(Engine::Groq(key));
        }
    }
    if cfg!(windows) && pick("windows") {
        return Ok(Engine::Windows);
    }
    // OpenRouter : seulement avec un modèle audio choisi (ou demandé).
    if pick("openrouter") && (env("CLAW_VOICE_MODEL").is_some() || wanted.is_some()) {
        if let Some(key) = key("OPENROUTER_API_KEY") {
            return Ok(Engine::OpenRouter(key));
        }
    }
    if wanted.as_deref() == Some("openai") {
        if let Some(key) = key("OPENAI_API_KEY") {
            return Ok(Engine::OpenAi(key));
        }
    }
    Err(match wanted {
        Some(wanted) => format!("moteur de dictée « {wanted} » indisponible (clé ou commande manquante)"),
        None => "aucun moteur de transcription : réglez GROQ_API_KEY (Whisper, palier gratuit), CLAW_WHISPER_MODEL (whisper.cpp local), CLAW_VOICE_MODEL (modèle audio d'OpenRouter) ou CLAW_VOICE_COMMAND".to_string(),
    })
}

/// Modèle audio d'`OpenRouter` : `CLAW_VOICE_MODEL`, sinon le premier modèle
/// gratuit du catalogue qui écoute l'audio.
fn openrouter_model() -> String {
    if let Some(model) = env("CLAW_VOICE_MODEL") {
        return model;
    }
    crate::models::catalog(false)
        .ok()
        .and_then(|(catalog, _)| {
            catalog
                .models
                .iter()
                .find(|model| model.supports_audio && model.is_free())
                .map(|model| model.id.clone())
        })
        .unwrap_or_else(|| "nvidia/nemotron-3-nano-omni-30b-a3b-reasoning:free".to_string())
}

/// Corps `multipart/form-data` : champs texte et le fichier `file`.
fn multipart(fields: &[(&str, String)], audio: &[u8]) -> (String, Vec<u8>) {
    let boundary = format!("claw{}", std::process::id());
    let mut body = Vec::new();
    for (name, value) in fields {
        body.extend_from_slice(
            format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n")
                .as_bytes(),
        );
    }
    body.extend_from_slice(
        format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"dictee.wav\"\r\nContent-Type: audio/wav\r\n\r\n")
            .as_bytes(),
    );
    body.extend_from_slice(audio);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    (format!("multipart/form-data; boundary={boundary}"), body)
}

fn whisper_api(url: &str, key: &str, model: &str, audio: &[u8]) -> Result<String, String> {
    let mut fields = vec![
        ("model", model.to_string()),
        ("response_format", "text".to_string()),
    ];
    if let Some(language) = language() {
        fields.push(("language", language));
    }
    let (content_type, body) = multipart(&fields, audio);
    let (status, text) = tools::http_post(
        url,
        &[("Authorization".to_string(), format!("Bearer {key}"))],
        &content_type,
        body,
        Duration::from_secs(60),
    )?;
    if status == 200 {
        Ok(text)
    } else {
        Err(format!(
            "HTTP {status} : {}",
            text.chars().take(300).collect::<String>()
        ))
    }
}

fn temp_wav(audio: &[u8]) -> Result<PathBuf, String> {
    let path = std::env::temp_dir().join(format!("claw-dictee-{}.wav", std::process::id()));
    std::fs::write(&path, audio).map_err(|error| error.to_string())?;
    Ok(path)
}

fn run_command(command: &str, file: &Path) -> Result<String, String> {
    let line = if command.contains("{file}") {
        command.replace("{file}", &file.display().to_string())
    } else {
        format!("{command} \"{}\"", file.display())
    };
    let output = if cfg!(windows) {
        Command::new("cmd")
            .args(["/C", &line])
            .stdin(Stdio::null())
            .output()
    } else {
        Command::new("sh")
            .args(["-c", &line])
            .stdin(Stdio::null())
            .output()
    }
    .map_err(|error| error.to_string())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

/// Reconnaissance vocale de Windows (`System.Speech`, dictée libre) dans la
/// langue de la dictée si elle est installée, sinon la première disponible.
fn windows_recognition(file: &Path) -> Result<String, String> {
    let language = language().unwrap_or_default().replace('\'', "");
    let file = file.display().to_string().replace('\'', "''");
    let script = format!(
        "Add-Type -AssemblyName System.Speech; \
         $all = [System.Speech.Recognition.SpeechRecognitionEngine]::InstalledRecognizers(); \
         $info = $all | Where-Object {{ $_.Culture.TwoLetterISOLanguageName -eq '{language}' }} | Select-Object -First 1; \
         if (-not $info) {{ $info = $all | Select-Object -First 1 }}; \
         if (-not $info) {{ [Console]::Error.WriteLine('aucune reconnaissance vocale installée dans Windows'); exit 2 }}; \
         $engine = New-Object System.Speech.Recognition.SpeechRecognitionEngine($info); \
         $engine.LoadGrammar((New-Object System.Speech.Recognition.DictationGrammar)); \
         $engine.SetInputToWaveFile('{file}'); \
         $parts = @(); \
         while ($true) {{ $result = $engine.Recognize(); if ($null -eq $result) {{ break }}; $parts += $result.Text }}; \
         $engine.Dispose(); \
         [Console]::OutputEncoding = [System.Text.Encoding]::UTF8; \
         [Console]::Out.Write(($parts -join ' '))"
    );
    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("powershell : {error}"))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

/// Texte d'un enregistrement WAV.
pub(crate) fn transcribe(audio: &[u8]) -> Result<String, String> {
    let engine = engine()?;
    let text = match &engine {
        Engine::Command(command) => {
            let file = temp_wav(audio)?;
            let result = run_command(command, &file);
            let _ = std::fs::remove_file(&file);
            result?
        }
        Engine::Whisper(model) => {
            let file = temp_wav(audio)?;
            let program = env("CLAW_WHISPER_BIN").unwrap_or_else(|| "whisper-cli".to_string());
            let mut command = Command::new(program);
            command.args(["-m", model, "-nt", "-f"]).arg(&file);
            if let Some(language) = language() {
                command.args(["-l", &language]);
            }
            let output = command.stdin(Stdio::null()).output();
            let _ = std::fs::remove_file(&file);
            let output = output.map_err(|error| format!("whisper-cli : {error}"))?;
            if !output.status.success() {
                return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
            }
            String::from_utf8_lossy(&output.stdout).into_owned()
        }
        Engine::Windows => {
            let file = temp_wav(audio)?;
            let result = windows_recognition(&file);
            let _ = std::fs::remove_file(&file);
            result?
        }
        Engine::Groq(key) => whisper_api(
            "https://api.groq.com/openai/v1/audio/transcriptions",
            key,
            &env("CLAW_VOICE_MODEL").unwrap_or_else(|| "whisper-large-v3-turbo".to_string()),
            audio,
        )?,
        Engine::OpenAi(key) => whisper_api(
            &format!(
                "{}/audio/transcriptions",
                env("OPENAI_BASE_URL").unwrap_or_else(|| "https://api.openai.com/v1".to_string())
            ),
            key,
            &env("CLAW_VOICE_MODEL").unwrap_or_else(|| "whisper-1".to_string()),
            audio,
        )?,
        Engine::OpenRouter(key) => {
            let language = language()
                .map(|code| format!(" The speech is in language code \"{code}\"."))
                .unwrap_or_default();
            let body = json!({
                "model": openrouter_model(),
                "temperature": 0,
                "messages": [{
                    "role": "user",
                    "content": [
                        { "type": "text", "text": format!("Transcribe this audio recording exactly, word for word.{language} Reply with the transcription only: no translation, no comment, no quotes.") },
                        { "type": "input_audio", "input_audio": { "data": runtime::attachments::encode_base64(audio), "format": "wav" } }
                    ]
                }]
            });
            let (status, text) = tools::http_post(
                &format!("{}/chat/completions", api::read_openrouter_base_url()),
                &[("Authorization".to_string(), format!("Bearer {key}"))],
                "application/json",
                body.to_string().into_bytes(),
                Duration::from_secs(90),
            )?;
            if status != 200 {
                return Err(format!(
                    "HTTP {status} : {}",
                    text.chars().take(300).collect::<String>()
                ));
            }
            serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|value| {
                    value["choices"][0]["message"]["content"]
                        .as_str()
                        .map(str::to_string)
                })
                .ok_or("réponse sans transcription")?
        }
    };
    let text = clean(&text);
    if text.is_empty() {
        Err("rien n'a été entendu".to_string())
    } else {
        Ok(text)
    }
}

/// Transcription d'une ligne : espaces réduites, guillemets et balises de
/// réflexion retirés.
fn clean(text: &str) -> String {
    let text = match text.rfind("</think>") {
        Some(end) => &text[end + "</think>".len()..],
        None => text,
    };
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_matches(|character: char| matches!(character, '"' | '«' | '»' | '“' | '”'))
        .trim()
        .to_string()
}

// --- /voice ---------------------------------------------------------------------

fn save(enabled: bool, mode: Option<Mode>) -> Result<(), String> {
    let current = settings();
    let mode = mode
        .or(current.map(|settings| settings.mode))
        .unwrap_or(Mode::Hold);
    let auto_submit = current.is_some_and(|settings| settings.auto_submit);
    crate::extra_commands::set_user_setting(
        "voice",
        json!({
            "enabled": enabled,
            "mode": if mode == Mode::Tap { "tap" } else { "hold" },
            "autoSubmit": auto_submit,
        }),
    )
    .map(|_| ())
}

fn status() -> String {
    let mut lines = vec![match settings() {
        Some(settings) => format!(
            "Dictée vocale : activée ({})",
            if settings.mode == Mode::Tap {
                "tap : Espace dans une saisie vide démarre, Espace arrête"
            } else {
                "hold : maintenez Espace pour parler, relâchez pour arrêter"
            }
        ),
        None => "Dictée vocale : désactivée (/voice pour l'activer)".to_string(),
    }];
    lines.push(format!(
        "  Micro            {}",
        input_device().unwrap_or_else(|| "aucun trouvé".to_string())
    ));
    lines.push(format!(
        "  Transcription    {}",
        engine().map_or_else(|error| error, |engine| engine.label())
    ));
    if let Some(language) = language() {
        lines.push(format!("  Langue           {language}"));
    }
    lines.push(
        "  Échap annule une dictée ; /voice test <fichier.wav> essaie la transcription".to_string(),
    );
    lines.join("\n")
}

/// `/voice` : bascule ; `/voice hold|tap|off` ; `/voice status` ;
/// `/voice test <fichier.wav>` transcrit un fichier.
pub(crate) fn command(args: Option<&str>) -> String {
    let args = args.map(str::trim).unwrap_or_default();
    let (first, rest) = args.split_once(char::is_whitespace).unwrap_or((args, ""));
    let result = match first.to_lowercase().as_str() {
        "" => {
            let enable = settings().is_none();
            save(enable, None).map(|()| status())
        }
        "hold" => save(true, Some(Mode::Hold)).map(|()| status()),
        "tap" => save(true, Some(Mode::Tap)).map(|()| status()),
        "off" | "non" => save(false, None).map(|()| status()),
        "status" | "etat" | "état" => Ok(status()),
        "test" => {
            let file = rest.trim().trim_matches('"');
            if file.is_empty() {
                return "Usage : /voice test <fichier.wav>".to_string();
            }
            std::fs::read(file)
                .map_err(|error| format!("{file} : {error}"))
                .and_then(|audio| transcribe(&audio))
                .map(|text| format!("Transcription : {text}"))
        }
        _ => return "Usage : /voice [hold|tap|off|status|test <fichier.wav>]".to_string(),
    };
    result.unwrap_or_else(|error| format!("Dictée : {error}"))
}

#[cfg(test)]
mod tests {
    use super::{clean, multipart, resample, wav};

    #[test]
    fn audio_is_resampled_and_written_as_wav() {
        let samples: Vec<f32> = (0..48_000)
            .map(|index| if index % 2 == 0 { 0.5 } else { -0.5 })
            .collect();
        let resampled = resample(&samples, 48_000, 16_000);
        assert_eq!(resampled.len(), 16_000);
        let file = wav(&resampled, 16_000);
        assert_eq!(&file[0..4], b"RIFF");
        assert_eq!(&file[8..16], b"WAVEfmt ");
        assert_eq!(file.len(), 44 + 32_000);
        assert_eq!(
            u32::from_le_bytes([file[24], file[25], file[26], file[27]]),
            16_000
        );
    }

    #[test]
    fn transcriptions_are_cleaned_and_forms_built() {
        assert_eq!(
            clean("<think>hmm</think>\n  « Bonjour   Claw »  "),
            "Bonjour Claw"
        );
        assert_eq!(clean("\"liste les fichiers\"\n"), "liste les fichiers");
        let (content_type, body) = multipart(&[("model", "whisper".to_string())], b"RIFF");
        assert!(content_type.starts_with("multipart/form-data; boundary=claw"));
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains("name=\"model\"\r\n\r\nwhisper"));
        assert!(text.contains("filename=\"dictee.wav\""));
    }
}
