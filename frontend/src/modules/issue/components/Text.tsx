import { Fragment } from "react";

import { dateTimeSeconds, millis, parseTimestamp } from "modules/issue/utils/dates";
import { djStr } from "modules/issue/utils/python";

/** Django's `linebreaks` filter: paragraphs on blank lines, `<br>` for single newlines. */
export function Linebreaks({ value }: { value: unknown }) {
  const text = djStr(value).replace(/\r\n|\r/g, "\n");
  return text.split(/\n{2,}/).map((paragraph, i) => (
    <p key={i}>
      <LinebreaksBr text={paragraph} />
    </p>
  ));
}

/** Django's `linebreaksbr` filter. */
export function LinebreaksBr({ text }: { text: string }) {
  return text
    .replace(/\r\n|\r/g, "\n")
    .split("\n")
    .map((line, i) => (
      <Fragment key={i}>
        {i > 0 && <br />}
        {line}
      </Fragment>
    ));
}

/** Upstream's `timestamp_with_millis` filter; the value as it is when it isn't a timestamp. */
export function TimestampWithMillis({ value }: { value: unknown }) {
  const date = value instanceof Date ? value : parseTimestamp(value);
  if (!date) {
    return <>{djStr(value)}</>;
  }
  return (
    <span className="whitespace-nowrap">
      {dateTimeSeconds(date)}.<span className="text-xs">{millis(date)}</span>
    </span>
  );
}
