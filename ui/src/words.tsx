// Words the server and the screens write with code spans — "`a` feeding `b`
// would close a cycle", "`x` is taken" — drawn with each backticked span as
// code, never with its backticks. The text itself is unchanged: what an
// assistive technology hears is the words, and a title or a copy keeps them.
import { Fragment } from 'react';

/** `text`, each span between two backticks drawn as code; a backtick with no partner stays as it is. */
export function Words({ text }: { text: string }) {
  const parts = text.split('`');
  // An even count of parts means one backtick has no partner: the last one stays literal.
  const unmatched = parts.length % 2 === 0;
  return (
    <>
      {parts.map((part, i) => {
        if (i % 2 === 0) return <Fragment key={i}>{part}</Fragment>;
        if (unmatched && i === parts.length - 1) return <Fragment key={i}>{`\`${part}`}</Fragment>;
        return <code key={i}>{part}</code>;
      })}
    </>
  );
}
