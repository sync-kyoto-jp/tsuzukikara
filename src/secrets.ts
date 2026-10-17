// Masks strings that look like credentials in the conversation preview. This is a display
// safeguard, not a guarantee: history files may contain secrets in forms these patterns miss.

const PATTERNS: RegExp[] = [
  /\bsk-[A-Za-z0-9_-]{16,}/g, // OpenAI / Anthropic style keys
  /\bgh[pousr]_[A-Za-z0-9]{20,}/g, // GitHub tokens
  /\bgithub_pat_[A-Za-z0-9_]{20,}/g,
  /\bxox[abprs]-[A-Za-z0-9-]{10,}/g, // Slack tokens
  /\bAKIA[0-9A-Z]{16}\b/g, // AWS access key ids
  /\bAIza[0-9A-Za-z_-]{30,}/g, // Google API keys
  /\beyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}/g, // JWTs
  /-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z ]*PRIVATE KEY-----/g,
  /\b(Bearer\s+)[A-Za-z0-9._~+/-]{16,}=*/gi,
  /\b((?:password|passwd|secret|token|api[_-]?key)\s*[:=]\s*)["']?[^\s"']{6,}/gi,
];

/** Replaces secret-like substrings with `••••` followed by the localized `label`. */
export function maskSecrets(text: string, label: string): { text: string; masked: number } {
  let masked = 0;
  let out = text;
  for (const re of PATTERNS) {
    out = out.replace(re, (match, prefix?: string) => {
      masked++;
      const keep = typeof prefix === "string" && match.startsWith(prefix) ? prefix : "";
      return `${keep}••••${label}`;
    });
  }
  return { text: out, masked };
}
