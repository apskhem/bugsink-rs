import { Link, useSearchParams } from "react-router";

import { formatNumber } from "common/utils/format";

import { PageIcon } from "./icons";

/** Upstream's issue lists page by 250. */
export const PAGE_SIZE = 250;

/**
 * Upstream's `UncountablePaginator` footer: first/previous/next links (`?page=`, other parameters kept) and the range
 * shown; there's a next page when this one is full.
 */
export default function Pagination({ page, count }: { page: number; count: number }) {
  const [searchParams] = useSearchParams();
  const startIndex = (page - 1) * PAGE_SIZE + 1;
  const endIndex = (page - 1) * PAGE_SIZE + count;

  function toPage(n: number) {
    const params = new URLSearchParams(searchParams);
    params.set("page", String(n));
    return `?${params.toString()}`;
  }

  return (
    <div className="flex ml-2">
      {page > 1 ? (
        <>
          <Link to={toPage(1)} className="inline-flex" title="First page">
            <PageIcon which="first" />
          </Link>
          <Link to={toPage(page - 1)} className="inline-flex" title="Previous page">
            <PageIcon which="previous" />
          </Link>
        </>
      ) : (
        <>
          <PageIcon which="first" disabled />
          <PageIcon which="previous" disabled />
        </>
      )}{" "}
      {count > 0
        ? `Issues ${formatNumber(startIndex)} – ${formatNumber(endIndex)}`
        : page > 1
          ? `Less than ${startIndex} Issues`
          : "0 Issues"}{" "}
      {count === PAGE_SIZE ? (
        <Link to={toPage(page + 1)} className="inline-flex" title="Next page">
          <PageIcon which="next" />
        </Link>
      ) : (
        <PageIcon which="next" disabled />
      )}
    </div>
  );
}
