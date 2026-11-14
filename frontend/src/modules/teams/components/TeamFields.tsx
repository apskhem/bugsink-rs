import { InputField, SelectField } from "common/components/FormField";
import type { FieldErrors } from "services/http";
import { TEAM_VISIBILITIES, type TeamForm } from "services/teams";

type Props = {
  value: TeamForm;
  onChange: (value: TeamForm) => void;
  errors: FieldErrors;
};

/** `TeamForm`'s fields: name and visibility. */
export default function TeamFields({ value, onChange, errors }: Props) {
  return (
    <>
      <InputField
        label="Name"
        type="text"
        name="name"
        maxLength={255}
        required
        value={value.name}
        onChange={(event) => onChange({ ...value, name: event.target.value })}
        errors={errors["name"]}
      />
      <SelectField
        label="Visibility"
        name="visibility"
        required
        value={value.visibility}
        onChange={(event) => onChange({ ...value, visibility: Number(event.target.value) })}
        errors={errors["visibility"]}
        helpText="Which users can see this team and its issues?"
      >
        {TEAM_VISIBILITIES.map((choice) => (
          <option key={choice.value} value={choice.value}>
            {choice.label}
          </option>
        ))}
      </SelectField>
    </>
  );
}
