import { useState, type FormEvent } from "react";
import { useNavigate, useParams } from "react-router";

import { LoadError } from "common/components/ErrorPage";
import { FormErrors } from "common/components/FormField";
import { routes } from "common/constants/routes";
import { useLoad } from "common/hooks/useLoad";
import { useTitle } from "common/hooks/useTitle";
import { useSession } from "contexts/session";
import { accountsApi } from "services/accounts";
import { isStatus } from "services/http";

import AccountBox from "../components/AccountBox";
import BareNotFound from "../components/BareNotFound";
import { submitErrors } from "../utils/errors";

/** Upstream's `Http404` for an unknown or expired token ("a special page might be prettier"). */
const INVALID_TOKEN = "Invalid or expired token";

/** Upstream's `users/confirm_email.html`: a button (a POST) confirms the address and logs in. */
export default function ConfirmEmail() {
  const { token = "" } = useParams();
  const { setUser } = useSession();
  const navigate = useNavigate();
  const checked = useLoad(() => accountsApi.checkConfirmEmailToken({ params: { token } }), [token]);
  const [invalid, setInvalid] = useState(false);
  const [errors, setErrors] = useState<string[]>();

  const notFound = invalid || (checked.state === "failed" && isStatus(checked.error, 404));
  useTitle(notFound || checked.state !== "loaded" ? null : "Confirm email");

  if (notFound) {
    return <BareNotFound message={INVALID_TOKEN} />;
  }
  if (checked.state === "failed") {
    return <LoadError error={checked.error} />;
  }
  if (checked.state === "loading") {
    return null;
  }

  async function confirm(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    try {
      setUser(await accountsApi.confirmEmail(undefined, { params: { token } }));
      await navigate(routes.home());
    } catch (error) {
      if (isStatus(error, 404)) {
        setInvalid(true);
      } else {
        setErrors(submitErrors(error)["__all__"]);
      }
    }
  }

  return (
    <AccountBox>
      <div className="mb-8">Confirm your email address by clicking the button below.</div>

      <form method="post" onSubmit={(event) => void confirm(event)}>
        <FormErrors errors={errors} />
        <button className="bg-slate-800 font-medium p-2 md:p-4 text-white uppercase w-full">Confirm</button>
      </form>
    </AccountBox>
  );
}
