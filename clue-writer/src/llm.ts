// Shared streaming + structured-output plumbing for every thinking-model
// Claude call in the pipeline.
//
// Why streaming: max_tokens bounds thinking + output COMBINED, and the
// thinking-heavy steps (clue writing, QA, ideation — especially on themed
// grids) were observed to spend most of a 16k budget thinking and truncate
// the JSON mid-string. The fix is a 32k budget — but the SDK refuses
// non-streaming requests whose worst-case duration estimate (max_tokens *
// 3600 / 128000 seconds) exceeds 10 minutes, which 32k does. Streaming +
// finalMessage() behaves identically to a blocking call from the caller's
// perspective; real requests take well under a minute.
//
// Why a shared helper: the failure modes (truncated JSON throw, missing
// parsed_output) are identical across call sites, and the actionable hints
// belong in one place.

import type Anthropic from "@anthropic-ai/sdk";

/** Generous combined thinking+output budget. Tokens are only billed as used. */
export const STRUCTURED_MAX_TOKENS = 32000;

export interface LlmUsage {
  input: number;
  output: number;
  cacheRead: number;
  cacheWrite: number;
}

/**
 * Run a structured-output request via streaming, wait for the full response,
 * and validate the parsed output against the given zod schema. Throws with
 * actionable hints on truncation.
 */
export async function streamStructured<T>(
  client: Anthropic,
  params: Parameters<Anthropic["messages"]["stream"]>[0],
  schema: { parse: (v: unknown) => T },
  step: string,
): Promise<{ output: T; usage: LlmUsage }> {
  let response: Awaited<ReturnType<ReturnType<Anthropic["messages"]["stream"]>["finalMessage"]>>;
  try {
    response = await client.messages.stream(params).finalMessage();
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err);
    const hint = /parse structured output/i.test(msg)
      ? " (the JSON was likely truncated — the model spent the token budget thinking; raise the max_tokens for this step)"
      : "";
    throw new Error(`${step}: ${msg}${hint}`);
  }
  const raw = (response as { parsed_output?: unknown }).parsed_output;
  if (!raw) {
    const hint =
      response.stop_reason === "max_tokens"
        ? " Truncated by max_tokens — the model spent the budget thinking; raise the max_tokens for this step."
        : "";
    throw new Error(`${step} returned no structured output (stop_reason: ${response.stop_reason}).${hint}`);
  }
  const u = response.usage;
  return {
    output: schema.parse(raw),
    usage: {
      input: u.input_tokens,
      output: u.output_tokens,
      cacheRead: u.cache_read_input_tokens ?? 0,
      cacheWrite: u.cache_creation_input_tokens ?? 0,
    },
  };
}
