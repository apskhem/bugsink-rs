import { Navigate, useParams } from "react-router";

import Load from "common/components/Load";
import { routes } from "common/constants/routes";
import { useLoad } from "common/hooks/useLoad";
import { issueApi } from "services/issue";

/** Upstream's `event_by_id`: a link to an event by its id, redirecting to its issue's page. */
export default function EventById() {
  const { eventId = "" } = useParams();
  const loaded = useLoad(() => issueApi.eventById({ params: { eventId } }), [eventId]);
  return <Load loaded={loaded}>{(event) => <Navigate to={routes.event(event.issue_id, event.id)} replace />}</Load>;
}
