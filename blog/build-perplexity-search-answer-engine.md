# Build a Perplexity-Style Search Answer Engine in 50 Lines (with Citations, BYOK)

> fastCRW v0.7.0 ships answer: true on /v1/search — one call gives you a synthesized answer plus validated citations. Full Python and TypeScript tutorial.

**Published:** 2026-05-14  
**Updated:** 2026-05-14  
**Canonical:** https://fastcrw.com/blog/build-perplexity-search-answer-engine

---

Perplexity made one product decision that turned a search engine into a category-defining product: *show the answer, cite the sources, let the reader verify*. Behind it is a textbook RAG pipeline — search, scrape top results, feed to an LLM, return answer plus structured citations.

That pipeline is a five-stage system to build yourself. Or one call to `POST /v1/search` with `answer: true` on fastCRW v0.7.0.

This tutorial wires it up end-to-end. By the end you'll have a working *ask me anything* CLI in Python (~35 lines) and a Next.js API route in TypeScript (~50 lines), both with citations, both BYOK, both costing under $0.001 per question with DeepSeek.

## What "search-answer" Actually Means

Three usage patterns blur together; clarify them before building:

| Pattern | Use case | API |
| --- | --- | --- |
| Raw search | You want URLs, not answers (agent picks next URL to scrape) | `/v1/search` without LLM fields |
| Search-answer | You want a sourced answer to a question, displayed in a UI | `/v1/search` with `answer: true` |
| DIY RAG | You have a private corpus, need a vector store + retriever | Not search-answer — use scrape + embed + retrieve |

Search-answer is for the second case: *fresh public web* + *question* = *answer with citations*. If your data is private, build DIY RAG. If you need URLs to feed an agent, use raw search.

## 1. First Call — One curl Command

```
curl -X POST https://api.fastcrw.com/v1/search \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $CRW_API_KEY" \
  -d "{
    \"query\": \"what is the Rust borrow checker\",
    \"limit\": 5,
    \"answer\": true,
    \"answerTopN\": 3,
    \"answerPrompt\": \"Answer in three sentences, technical tone.\",
    \"scrapeOptions\": { \"formats\": [\"markdown\"] },
    \"llmProvider\": \"openai-compatible\",
    \"llmModel\": \"deepseek-chat\",
    \"baseUrl\": \"https://api.deepseek.com/v1\",
    \"llmApiKey\": \"$DEEPSEEK_API_KEY\"
  }"
```

The engine: searches → scrapes top 3 → feeds to DeepSeek → returns answer + citations. One round trip, one credit reservation, one bill.

## 2. Response Anatomy

```
{
  "success": true,
  "data": {
    "results": [
      { "url": "...", "title": "...", "markdown": "..." },
      ...
    ],
    "answer": "The Rust borrow checker is a static analysis component of the compiler that enforces the language's ownership and borrowing rules at compile time [1]. It tracks how references to values flow through a program and rejects code where the same value could be mutated and read concurrently, eliminating data races without runtime overhead [2]. The result is memory safety without a garbage collector [3].",
    "citations": [
      { "url": "https://doc.rust-lang.org/book/ch04-02-references-and-borrowing.html", "title": "References and Borrowing - The Rust Book", "position": 1 },
      { "url": "https://en.wikipedia.org/wiki/Rust_(programming_language)", "title": "Rust (programming language)", "position": 2 },
      { "url": "https://blog.rust-lang.org/...", "title": "Non-lexical lifetimes", "position": 3 }
    ],
    "llmUsage": {
      "inputTokens": 4280,
      "outputTokens": 96,
      "totalTokens": 4376,
      "estimatedCostUsd": 0.00126,
      "model": "deepseek-chat",
      "provider": "openai"
    },
    "warnings": []
  }
}
```

Key fields:

- `data.answer` — the synthesized text with inline `[1]`, `[2]` markers.
- `data.citations[]` — validated source list, indexable by `position`.
- `data.results[]` — full scraped results, so you can render raw sources if needed.
- `data.llmUsage` — token counts and cost estimate.
- `data.warnings[]` — non-fatal issues (e.g., a result that failed to scrape).

## 3. Citation Validation (the part nobody talks about)

LLMs hallucinate citations. A model can invent a source ID, cite a position that doesn't exist, or duplicate the same source three times. If you render `[1]`, `[2]` markers in a UI without checking that they map to real sources, your product looks credible while quietly lying.

fastCRW validates citations server-side before returning them:

- **Fabricated source IDs dropped.** A citation pointing to a source outside the result set is removed.
- **Positions clamped.** If the LLM cites `[7]` but only 3 results exist, the citation is dropped (not renumbered into a wrong source).
- **Duplicates deduped.** Same URL cited twice collapses to one entry.
- **Capped at 20.** Even if the model cites obsessively, the response stays bounded.

You don't write any of that logic. It's in `crates/crw-extract/src/answer.rs` in the open-source engine.

## 4. Tuning answerTopN — Latency vs Grounding

| `answerTopN` | Latency | LLM cost / question | When to use |
| --- | --- | --- | --- |
| 3 | ~6–10s | ~$0.001 | Fast Q&A, narrow factual queries |
| 5 (default) | ~10–15s | ~$0.002 | Balanced — start here |
| 10 (max) | ~15–25s | ~$0.004 | Comparative or "what are the options" queries |

Latency is dominated by the scrape phase, not the LLM. Each scrape adds a network round-trip to a different upstream site. Setting `answerTopN` higher than your actual evidence need wastes both time and tokens.

## 5. Python — A 35-line "Ask Me Anything" CLI

```
import os

CRW_URL = "https://api.fastcrw.com/v1/search"
CRW_KEY = os.environ["CRW_API_KEY"]
DEEPSEEK_KEY = os.environ["DEEPSEEK_API_KEY"]

def ask(query: str, top_n: int = 3) -> dict:
    r = httpx.post(
        CRW_URL,
        json={
            "query": query,
            "limit": 5,
            "answer": True,
            "answerTopN": top_n,
            "answerPrompt": "Answer in two to four sentences, plain English.",
            "scrapeOptions": {"formats": ["markdown"]},
            "llmProvider": "openai-compatible",
            "llmModel": "deepseek-chat",
            "baseUrl": "https://api.deepseek.com/v1",
            "llmApiKey": DEEPSEEK_KEY,
        },
        headers={"Authorization": f"Bearer {CRW_KEY}"},
        timeout=120,
    )
    r.raise_for_status()
    return r.json()["data"]

def render(data: dict) -> str:
    lines = [data["answer"], "", "Sources:"]
    for c in data["citations"]:
        lines.append(f"  [{c['position']}] {c['title']} — {c['url']}")
    cost = data.get("llmUsage", {}).get("estimatedCostUsd", 0)
    lines.append(f"\n(cost: ~\${cost:.5f})")
    return "\n".join(lines)

if __name__ == "__main__":
    query = " ".join(sys.argv[1:]) or "what is fastCRW"
    print(render(ask(query)))
```

Save as `ama.py`, run `uv run python ama.py "explain async runtimes in Rust"`, get a sourced answer in 10 seconds for under a tenth of a cent.

## 6. TypeScript — Next.js API Route in 50 Lines

```
// app/api/ask/route.ts

const CRW_URL = "https://api.fastcrw.com/v1/search";

interface Citation {
  url: string;
  title: string;
  position: number;
}

interface AnswerResponse {
  answer: string;
  citations: Citation[];
  llmUsage?: { estimatedCostUsd?: number };
}

export async function POST(req: Request) {
  const { query, topN = 3 } = await req.json();
  if (typeof query !== "string" || !query.trim()) {
    return NextResponse.json({ error: "query required" }, { status: 400 });
  }

  const upstream = await fetch(CRW_URL, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${process.env.CRW_API_KEY}`,
    },
    body: JSON.stringify({
      query,
      limit: 5,
      answer: true,
      answerTopN: topN,
      answerPrompt: "Answer in two to four sentences.",
      scrapeOptions: { formats: ["markdown"] },
      llmProvider: "openai-compatible",
      llmModel: "deepseek-chat",
      baseUrl: "https://api.deepseek.com/v1",
      llmApiKey: process.env.DEEPSEEK_API_KEY,
    }),
  });

  if (!upstream.ok) {
    return NextResponse.json(
      { error: `upstream ${upstream.status}` },
      { status: 502 }
    );
  }

  const { data } = (await upstream.json()) as { data: AnswerResponse };
  return NextResponse.json({
    answer: data.answer,
    citations: data.citations,
    costUsd: data.llmUsage?.estimatedCostUsd ?? null,
  });
}
```

Pair it with a tiny React form that POSTs to `/api/ask` and you have a working Perplexity clone, citations included.

## 7. Rendering Citations in HTML

The answer contains inline `[1]`, `[2]` markers. To turn them into anchors:

```
function renderAnswerHtml(answer: string, citations: Citation[]): string {
  const byPosition = new Map(citations.map((c) => [c.position, c]));
  return answer.replace(/\[(\d+)\]/g, (match, idx) => {
    const c = byPosition.get(Number(idx));
    if (!c) return match;
    return `<sup><a href="${c.url}" target="_blank" rel="noopener" title="${c.title}">[${idx}]</a></sup>`;
  });
}
```

Because citations are server-validated, you can trust the map lookup — there will be no `[7]` in the answer if the citations array doesn't include position 7.

## 8. When to Use Search-Answer vs DIY RAG

Pick search-answer when:

- The data is on the **public web** and you don't already have an index.
- You need **freshness** (today's news, latest docs, just-shipped releases).
- You want a **single dependency** instead of a vector store + retriever + LLM stack.

Pick DIY RAG when:

- The data is **private** (internal wiki, customer support tickets, contracts).
- You re-query the same corpus many times and care about **cost per query** at scale (vector retrieval is cheaper than search-answer at 10k+ QPS).
- You need **cross-document reasoning** over a fixed corpus.

Both patterns coexist. Many production agents use search-answer for fresh public context and DIY RAG for private context, fused at prompt time.

## 9. Production Concerns

### Cache answers, not searches

Search results change. LLM answers over the same results don't. Hash `(query, top_n)` → cache the answer for a TTL based on how time-sensitive the topic is (5 min for news, 1 day for technical docs, 1 week for evergreen). Saves tokens and latency.

### Rate-limit by user, not by query

Each search-answer call is ~$0.002 in LLM tokens + 1 search credit + N scrape credits. A user looping a script can rack up cost fast. Enforce per-user QPS at your API gateway.

### The answer can be empty

If all top results fail to scrape (anti-bot, timeout), the engine returns an empty `answer` with `warnings` populated. Render the warnings; don't fall back to "the page says nothing."

### Prompt-injection is handled

fastCRW wraps scraped content in `=====UNTRUSTED:=====` delimiters. Adversarial pages can't redirect the LLM to ignore the query. Your `answerPrompt` is capped at 500 chars and treated as style guidance — it cannot override "answer the user's query using only the provided sources."

## What's Next

- [CRW vs Tavily vs Exa vs Perplexity head-to-head](/blog/crw-vs-tavily-exa-perplexity-search-answer-api)
- [Same DeepSeek setup applied to per-URL summaries](/blog/deepseek-web-scraping-byok-tutorial)
- [Search-answer endpoint docs](https://docs.fastcrw.com/search/)
- [Credit costs — LLM is BYOK](https://docs.fastcrw.com/credit-costs/)

## FAQ

### Is search-answer a RAG system?

Yes, it's a textbook public-web RAG pipeline (search → scrape → LLM with citations) packaged behind one API call. It is not a substitute for private-corpus RAG, where you need your own vector store and retriever.

### How does CRW prevent the LLM from hallucinating citations?

Citations are validated server-side. Fabricated source IDs (pointing outside the result set) are dropped, positions are clamped to the actual result range, duplicates are deduped, and the list is capped at 20. Hallucinated citations never reach your client.

### What's the latency of a typical answer query?

Roughly 10–15 seconds for answerTopN: 5 with DeepSeek. Latency is dominated by scrape time, not LLM time. Lower answerTopN to 3 for ~6–10 second responses on simple factual queries.

### Can I use search-answer without scraping?

Technically yes — omit scrapeOptions and the LLM will work from search snippets only. Quality drops sharply because snippets are ~150 chars each. The result is closer to summarized search descriptions than a sourced answer. Always include scrapeOptions in production.

### How is this different from Perplexity's API?

CRW is BYOK and open-source. You bring your own LLM key (Anthropic, OpenAI, DeepSeek, etc.) and pay your provider directly with no markup. The engine is open under AGPL-3.0, so you can self-host the entire pipeline. Perplexity bundles a fixed LLM, marks up tokens, and is closed-source.

### What's in the warnings array?

Non-fatal issues that affected the answer: which result URLs failed to scrape, whether content was truncated to fit maxCharsPerSource, whether some citations were dropped during validation. Render these in admin/debug UIs but typically hide them from end users unless the answer itself is empty.
