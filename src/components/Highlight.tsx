import { Fragment, ReactNode } from "react";
import { HIT_END, HIT_START } from "../api";

/** Renders backend snippets whose matches are wrapped in private-use marker characters.
 *  Text is rendered as React text nodes, never as HTML. */
export default function Highlight({ text }: { text: string }) {
  const parts: { text: string; hit: boolean }[] = [];
  let rest = text;
  while (rest.length) {
    const s = rest.indexOf(HIT_START);
    if (s < 0) {
      parts.push({ text: rest, hit: false });
      break;
    }
    const e = rest.indexOf(HIT_END, s);
    if (s > 0) parts.push({ text: rest.slice(0, s), hit: false });
    if (e < 0) {
      parts.push({ text: rest.slice(s + 1), hit: true });
      break;
    }
    parts.push({ text: rest.slice(s + 1, e), hit: true });
    rest = rest.slice(e + 1);
  }
  return (
    <>
      {parts.map((p, i) => (p.hit ? <mark key={i}>{p.text}</mark> : <Fragment key={i}>{p.text}</Fragment>))}
    </>
  );
}

/** Marks occurrences of query terms in plain text (used in the conversation preview). */
export function HighlightTerms({ text, terms }: { text: string; terms: string[] }) {
  const needles = terms.filter(Boolean).map((t) => t.toLowerCase());
  if (!needles.length) return <>{text}</>;
  const lower = text.toLowerCase();
  const out: ReactNode[] = [];
  let i = 0;
  let key = 0;
  while (i < text.length) {
    let best = -1;
    let len = 0;
    for (const n of needles) {
      const p = lower.indexOf(n, i);
      if (p >= 0 && (best < 0 || p < best)) {
        best = p;
        len = n.length;
      }
    }
    if (best < 0) {
      out.push(<Fragment key={key++}>{text.slice(i)}</Fragment>);
      break;
    }
    if (best > i) out.push(<Fragment key={key++}>{text.slice(i, best)}</Fragment>);
    out.push(<mark key={key++}>{text.slice(best, best + len)}</mark>);
    i = best + len;
  }
  return <>{out}</>;
}
