export const meta = {
  name: 'deep-research',
  description: 'Fan out web searches on a question, cross-check the sources and write a cited report',
  phases: [{ title: 'Plan' }, { title: 'Search' }, { title: 'Report' }],
}

const question = typeof args === 'string' ? args : (args && args.question) || ''
if (!question.trim()) return { error: 'usage: /deep-research <question>' }

phase('Plan')
const plan = await agent(
  `Plan a web research on this question: ${question}\n` +
    'Give 3 to 4 complementary search queries (different angles, recent and authoritative sources). Do not search yet.',
  {
    label: 'plan',
    schema: { type: 'object', required: ['queries'], properties: { queries: { type: 'array', items: { type: 'string' } } } },
  },
)
const queries = (plan && Array.isArray(plan.queries) && plan.queries.length ? plan.queries : [question]).slice(0, 4)

phase('Search')
const findings = await pipeline(queries, (query) =>
  agent(
    `Research this query with WebSearch, then read the two or three most relevant results with WebFetch: ${query}\n` +
      `It serves the question: ${question}\n` +
      'Return the facts you found, each with the URL of the page it comes from. Only report what the pages say.',
    {
      label: query,
      schema: {
        type: 'object',
        required: ['facts'],
        properties: {
          facts: {
            type: 'array',
            items: { type: 'object', required: ['fact', 'url'], properties: { fact: { type: 'string' }, url: { type: 'string' } } },
          },
        },
      },
    },
  ),
)
const facts = findings.filter(Boolean).flatMap((entry) => entry.facts || [])
if (!facts.length) return { question, queries, report: null, note: 'no source could be read' }

phase('Report')
const report = await agent(
  `Write a research report in the language of this question, answering it: ${question}\n` +
    'Use only the facts below. Cross-check them and say where sources disagree. Cite each claim with its URL in brackets, and end with the list of sources. Do not use tools.\n\n' +
    facts.map((entry, index) => `${index + 1}. ${entry.fact} (${entry.url})`).join('\n'),
  { label: 'report' },
)
return { question, queries, sources: [...new Set(facts.map((entry) => entry.url))], report }
