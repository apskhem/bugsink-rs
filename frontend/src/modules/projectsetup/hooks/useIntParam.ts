import { useParams } from "react-router";

/** A numeric path parameter (upstream's `<int:...>`); null when it isn't one, which is a 404. */
export function useIntParam(name: string): number | null {
  const value = useParams()[name];
  return value !== undefined && /^\d+$/.test(value) ? Number(value) : null;
}
