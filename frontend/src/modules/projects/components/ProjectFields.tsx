import { InputField, SelectField } from "common/components/FormField";
import { GROUPING_MECHANISMS, PROJECT_VISIBILITIES } from "services/projects";

/** `ProjectForm`'s fields as the new and edit pages both show them (the team, slug and DSN are theirs). */
export type ProjectFormState = {
  name: string;
  visibility: number;
  /** As typed: empty is "This field is required." */
  retention: string;
  grouping: string;
};

type FieldProps<T> = {
  value: T;
  onChange: (value: T) => void;
  errors: string[] | undefined;
};

export function NameField({ value, onChange, errors }: FieldProps<string>) {
  return (
    <InputField
      label="Name"
      type="text"
      name="name"
      maxLength={255}
      required
      value={value}
      onChange={(event) => onChange(event.target.value)}
      errors={errors}
    />
  );
}

export function VisibilityField({ value, onChange, errors }: FieldProps<number>) {
  return (
    <SelectField
      label="Visibility"
      name="visibility"
      required
      value={value}
      onChange={(event) => onChange(Number(event.target.value))}
      errors={errors}
      helpText="Which users can see this project and its issues?"
    >
      {PROJECT_VISIBILITIES.map((choice) => (
        <option key={choice.value} value={choice.value}>
          {choice.label}
        </option>
      ))}
    </SelectField>
  );
}

export function RetentionField({ value, onChange, errors }: FieldProps<string>) {
  return (
    <InputField
      label="Retention max event count"
      type="number"
      name="retention_max_event_count"
      min={0}
      required
      value={value}
      onChange={(event) => onChange(event.target.value)}
      errors={errors}
      helpText="The maximum number of events to store before evicting."
    />
  );
}

/** `transition`: the mechanism being moved away from, and until when (upstream: "... -- previous, transitioning"). */
export function GroupingField({
  value,
  onChange,
  errors,
  transition
}: FieldProps<string> & { transition?: { previous: string; until: string } | null }) {
  return (
    <SelectField
      label="Grouping mechanism"
      name="grouping_mechanism"
      required
      value={value}
      onChange={(event) => onChange(event.target.value)}
      errors={errors}
    >
      {GROUPING_MECHANISMS.map((choice) => (
        <option key={choice.value} value={choice.value}>
          {transition?.previous === choice.value
            ? `${choice.label} -- previous, transitioning until ${transition.until}`
            : choice.label}
        </option>
      ))}
    </SelectField>
  );
}

/** The form's number field as the API takes it: left out when empty (the API answers "This field is required."). */
export function retentionValue(retention: string): number | undefined {
  return retention.trim() === "" ? undefined : Number(retention);
}
