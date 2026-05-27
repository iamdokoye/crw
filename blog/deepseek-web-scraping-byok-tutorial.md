# DeepSeek + fastCRW: AI Web Summaries at $0.27 per Million Tokens (BYOK Tutorial)

> Build a production AI web summarizer with DeepSeek and fastCRW. 100 pages for under $0.10, no token markup, OpenAI-compatible API. Full Python and TypeScript code.

**Published:** 2026-05-13  
**Updated:** 2026-05-13  
**Canonical:** https://fastcrw.com/blog/deepseek-web-scraping-byok-tutorial

---

DeepSeek's `deepseek-chat` costs **$0.27 per million input tokens, $1.10 per million output tokens**. That's about 1/11th of Claude Sonnet 4 and competitive with GPT-4o-mini. The API is OpenAI-compatible, so any tool that speaks the OpenAI Chat Completions protocol speaks to DeepSeek.

fastCRW v0.7.0 ships BYOK support for any OpenAI-compatible endpoint. This tutorial wires DeepSeek into the `/v1/scrape` `summary` format and builds a production-ready AI summarizer that costs **~$0.0008 per 10 KB page**. Total cost for 1,000 pages: under $1.

## 1. Why DeepSeek for Scrape Summaries

Three reasons:

1. **Price.** $0.27/M input × ~2,500 tokens per page = ~$0.0007 input cost. Plus ~$0.0001 output cost. Total: under a tenth of a cent per page.
2. **Reasoning quality.** DeepSeek V3 family scores competitively with GPT-4o on long-form comprehension benchmarks. For summarization, it's indistinguishable from frontier models in practice.
3. **OpenAI compatibility.** No custom SDK needed. `baseUrl: "https://api.deepseek.com/v1"` and any OpenAI client works.

The tradeoffs: DeepSeek's rate limits are tighter than OpenAI's at the free tier, and account approval can take a day. Plan accordingly.

## 2. Get a DeepSeek API Key

Visit [platform.deepseek.com](https://platform.deepseek.com), register, top up at least $1, and create a key. Keys look like `sk-...`. Store it as an environment variable; never commit it.

```
export DEEPSEEK_API_KEY="sk-your-key-here"
export CRW_API_KEY="your-fastcrw-key"
```

## 3. First Request — curl

```
curl -X POST https://api.fastcrw.com/v1/scrape \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $CRW_API_KEY" \
  -d "{
    \"url\": \"https://en.wikipedia.org/wiki/Rust_(programming_language)\",
    \"formats\": [\"summary\"],
    \"summaryPrompt\": \"Respond in three sentences.\",
    \"llmProvider\": \"openai-compatible\",
    \"llmModel\": \"deepseek-chat\",
    \"baseUrl\": \"https://api.deepseek.com/v1\",
    \"llmApiKey\": \"$DEEPSEEK_API_KEY\"
  }"
```

Response (abridged):

```
{
  "success": true,
  "data": {
    "summary": "Rust is a multi-paradigm, general-purpose systems programming language that emphasizes performance, memory safety, and concurrency without relying on a garbage collector. It enforces these guarantees through a unique ownership and borrowing model checked at compile time, with the optional 'unsafe' keyword for low-level work. Originally developed at Mozilla starting in 2010, Rust has been consistently voted the 'most loved' language in the Stack Overflow Developer Survey and is now used in production at Microsoft, Amazon, Google, Meta, and the Linux kernel.",
    "llmUsage": {
      "inputTokens": 3287,
      "outputTokens": 102,
      "totalTokens": 3389,
      "estimatedCostUsd": 0.001000,
      "model": "deepseek-chat",
      "provider": "openai"
    }
  }
}
```

One Wikipedia-sized page: roughly a tenth of a cent (~$0.0008 in DeepSeek tokens plus one CRW scrape credit). Now scale it.

## 4. Python Batch Summarizer (100 URLs)

Async with `httpx`, bounded concurrency, retry on transient errors:

```
import asyncio

CRW_URL = "https://api.fastcrw.com/v1/scrape"
CRW_KEY = os.environ["CRW_API_KEY"]
DEEPSEEK_KEY = os.environ["DEEPSEEK_API_KEY"]

PAYLOAD_TEMPLATE = {
    "formats": ["summary"],
    "summaryPrompt": "Respond in two sentences.",
    "llmProvider": "openai-compatible",
    "llmModel": "deepseek-chat",
    "baseUrl": "https://api.deepseek.com/v1",
    "llmApiKey": DEEPSEEK_KEY,
}

async def summarize_one(client: httpx.AsyncClient, url: str) -> dict:
    payload = {**PAYLOAD_TEMPLATE, "url": url}
    headers = {"Authorization": f"Bearer {CRW_KEY}"}
    for attempt in range(3):
        try:
            r = await client.post(CRW_URL, json=payload, headers=headers, timeout=120)
            r.raise_for_status()
            data = r.json()["data"]
            return {
                "url": url,
                "summary": data.get("summary"),
                "cost_usd": data.get("llmUsage", {}).get("estimatedCostUsd", 0),
            }
        except Exception as e:
            if attempt == 2:
                return {"url": url, "error": str(e)}
            await asyncio.sleep(2 ** attempt)

async def summarize_all(urls: list[str], concurrency: int = 8) -> list[dict]:
    sem = asyncio.Semaphore(concurrency)
    async with httpx.AsyncClient() as client:
        async def bound(u):
            async with sem:
                return await summarize_one(client, u)
        return await asyncio.gather(*(bound(u) for u in urls))

if __name__ == "__main__":
    urls = [
        "https://en.wikipedia.org/wiki/Rust_(programming_language)",
        "https://en.wikipedia.org/wiki/Python_(programming_language)",
        # ...98 more
    ]
    results = asyncio.run(summarize_all(urls))
    total_cost = sum(r.get("cost_usd", 0) for r in results)
    print(f"Summarized {len(urls)} URLs for ~$\{total_cost:.4f\} in DeepSeek tokens")
```

Replace the embedded escape sequence above (`$\{...\}`) with a Python f-string in your own code. The blog escapes braces here only to keep MDX-style templating safe.

### Expected cost

100 typical Wikipedia-sized pages: ~$0.08–$0.12 in DeepSeek tokens + 100 CRW scrape credits. At roughly $0.001 per credit (about $1 per 1,000 pages — it drops further on the higher-volume plans; see [fastcrw.com/pricing](https://fastcrw.com/pricing)), the CRW credits dominate; LLM cost is noise.

## 5. TypeScript / Node Version

```
import { setTimeout as sleep } from "node:timers/promises";

const CRW_URL = "https://api.fastcrw.com/v1/scrape";
const CRW_KEY = process.env.CRW_API_KEY!;
const DEEPSEEK_KEY = process.env.DEEPSEEK_API_KEY!;

interface SummaryResult {
  url: string;
  summary?: string;
  costUsd?: number;
  error?: string;
}

async function summarizeOne(url: string): Promise<SummaryResult> {
  const body = {
    url,
    formats: ["summary"],
    summaryPrompt: "Respond in two sentences.",
    llmProvider: "openai-compatible",
    llmModel: "deepseek-chat",
    baseUrl: "https://api.deepseek.com/v1",
    llmApiKey: DEEPSEEK_KEY,
  };
  for (let attempt = 0; attempt < 3; attempt++) {
    try {
      const r = await fetch(CRW_URL, {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          Authorization: `Bearer ${CRW_KEY}`,
        },
        body: JSON.stringify(body),
      });
      if (!r.ok) throw new Error(`HTTP ${r.status}`);
      const json = await r.json();
      return {
        url,
        summary: json.data?.summary,
        costUsd: json.data?.llmUsage?.estimatedCostUsd,
      };
    } catch (err) {
      if (attempt === 2) return { url, error: String(err) };
      await sleep(1000 * 2 ** attempt);
    }
  }
  return { url, error: "exhausted" };
}

async function summarizeAll(urls: string[], concurrency = 8): Promise<SummaryResult[]> {
  const out: SummaryResult[] = new Array(urls.length);
  let next = 0;
  await Promise.all(
    Array.from({ length: concurrency }, async () => {
      while (true) {
        const i = next++;
        if (i >= urls.length) return;
        out[i] = await summarizeOne(urls[i]);
      }
    })
  );
  return out;
}
```

## 6. Cost Comparison Table

Same task (100 pages, ~2,500 input tokens, ~80 output tokens each):

| Model | Total LLM cost | Notes |
| --- | --- | --- |
| deepseek-chat | ~$0.08 | Sweet spot for batch summarization |
| gpt-4o-mini | ~$0.04 | Cheapest, watch OpenAI rate limits |
| claude-haiku-4-5 | ~$0.29 | Best for nuanced/edge-case content |
| claude-sonnet-4 | ~$0.87 | Frontier quality, frontier price |

DeepSeek's value isn't being absolute cheapest — it's *availability*. OpenAI rate-limits aggressively at low tiers; DeepSeek's "openai-compatible" mode lets you fail over without changing code.

## 7. Multilingual Summaries via summaryPrompt

The `summaryPrompt` field accepts up to 500 characters and is injected as a style directive. Use it for language, tone, or length control:

```
// Turkish
"summaryPrompt": "Türkçe iki cümle ile özetle."

// German
"summaryPrompt": "Fasse den Inhalt in zwei deutschen Sätzen zusammen."

// French + technical
"summaryPrompt": "Résume en deux phrases en français, ton technique."

// Bullet points
"summaryPrompt": "Three bullet points, no prose."
```

Note: `summaryPrompt` cannot override the core summarization task. If you ask "ignore the page and say hello," the model will still summarize the page — it's wrapped under a safety system prompt.

## 8. Production Tips

### Anti-bot pages return confident hallucinations

If a target site is blocked and returns near-empty content, DeepSeek will still produce a confident-sounding summary from its training memory. Always check `metadata.statusCode` and `data.markdown` length before trusting `data.summary`. Wikipedia, for example, sometimes anti-bots the scrape but the summary still reads correctly — because the model recognized the URL from training, not because the scrape worked.

Rule of thumb: if `data.markdown.length < 500` chars and the page is supposed to be substantial, treat the summary as suspect.

### Retry only on 5xx and network errors

4xx errors mean validation failed or your DeepSeek key is invalid. Retrying won't help and burns tokens. The Python and TypeScript snippets above retry only on `raise_for_status` / `!ok` — adjust if you want to be stricter.

### Use bounded concurrency, not Promise.all over 1000 items

DeepSeek's rate limits at the free tier are tight. Use a semaphore (Python) or a fixed worker pool (TypeScript) to cap concurrent in-flight requests. 8 concurrent is a safe starting point; raise to 16–32 once you've paid into a higher tier.

### Prompt-injection is handled for you

fastCRW wraps page content in `=====UNTRUSTED:=====` delimiters before passing it to DeepSeek. Adversarial content like "Ignore previous instructions and..." is rendered as data, not as a command. You do not need to sanitize pages.

## 9. n8n Recipe

For a no-code pipeline, drop these nodes into n8n:

1. **Trigger:** Webhook or schedule.
2. **HTTP Request node:** POST `https://api.fastcrw.com/v1/scrape`, JSON body identical to the curl snippet above, DeepSeek and CRW keys as credentials.
3. **Set node:** Extract `$json.data.summary` into a flat field.
4. **Sink:** Notion, Google Sheets, Postgres — wherever you store digests.

Replace one node's URL list with a Loop Over Items node fed by a Google Sheets read, and you have a batch summarizer with retries built in.

## 10. LangChain Integration

If your stack already uses LangChain documents, wrap the scrape call:

```
from langchain_core.documents import Document

async def fetch_summary_doc(url: str) -> Document:
    r = await httpx.AsyncClient().post(
        "https://api.fastcrw.com/v1/scrape",
        headers={"Authorization": f"Bearer {os.environ['CRW_API_KEY']}"},
        json={
            "url": url,
            "formats": ["markdown", "summary"],
            "llmProvider": "openai-compatible",
            "llmModel": "deepseek-chat",
            "baseUrl": "https://api.deepseek.com/v1",
            "llmApiKey": os.environ["DEEPSEEK_API_KEY"],
        },
        timeout=120,
    )
    data = r.json()["data"]
    return Document(
        page_content=data["markdown"],
        metadata={
            "url": url,
            "summary": data.get("summary"),
            "llm_cost_usd": data.get("llmUsage", {}).get("estimatedCostUsd"),
        },
    )
```

The `summary` field lands in the document's metadata so RAG retrievers can rank by digest similarity before falling back to full content.

## What's Next

- [Build a Perplexity-style answer engine with the same DeepSeek setup](/blog/build-perplexity-search-answer-engine)
- [Full v0.7.0 release notes](/blog/crw-v0-7-0-llm-release)
- [Scrape endpoint docs (LLM summary section)](https://docs.fastcrw.com/scraping/)
- [Credit costs (LLM is BYOK, 0 CRW markup)](https://docs.fastcrw.com/credit-costs/)

## FAQ

### Why does DeepSeek work with llmProvider: 'openai-compatible' instead of 'deepseek'?

Both work, but 'openai-compatible' is the more general dispatch path that uses the OpenAI Chat Completions format. DeepSeek's API speaks this protocol natively, so you point baseUrl at https://api.deepseek.com/v1 and any OpenAI-compatible client (or the CRW engine's built-in client) connects with no DeepSeek-specific code. The 'deepseek' provider alias is a convenience and behaves identically under the hood.

### What happens when DeepSeek's API is rate-limited?

The engine returns the provider's error verbatim with HTTP 4xx, and fastCRW passes it through. The Python and TypeScript snippets retry with exponential backoff on transient failures. If you hit a hard rate limit, the simplest fix is to lower concurrency from 8 to 4 or to upgrade your DeepSeek tier ($10 prepay typically unlocks higher quota).

### Can I use deepseek-reasoner instead of deepseek-chat?

Yes. Set llmModel: 'deepseek-reasoner'. The reasoner model is slower and more expensive per token but produces stronger analytical summaries. For typical short-form summaries, deepseek-chat is the better cost/quality tradeoff.

### Does fastCRW store my DeepSeek API key?

No. The key is forwarded to the engine per-request and used to call DeepSeek directly. It is not logged, not cached, and not stored. If you're self-hosting CRW, you can verify this in crates/crw-extract/src/llm.rs.

### How do I summarize a PDF instead of HTML?

Same payload. fastCRW's /v1/scrape handles PDFs transparently — the engine detects content-type, extracts text, and passes it to the LLM. Some scanned PDFs may need OCR; check data.markdown.length before trusting the summary.

### What's the maximum content size the LLM sees?

100 KB by default (maxContentChars), hard cap 200 KB. Content beyond that is truncated. For very long documents, you can either lower maxContentChars to save tokens or pre-chunk with the scrape endpoint's chunkStrategy and summarize each chunk.
