import { InputField } from "common/components/FormField";
import { configFields } from "modules/projectsetup/utils/messagingServices";
import type { FieldErrors } from "services/http";

/** A kind's config form, with `tailwind_formfield` for each field. */
export default function ConfigFields({
  kind,
  values,
  errors,
  onChange
}: {
  kind: string;
  values: Record<string, string>;
  errors: FieldErrors;
  onChange: (name: string, value: string) => void;
}) {
  return configFields(kind).map((field) => (
    <InputField
      key={field.name}
      label={field.label}
      errors={errors[field.name]}
      helpText={field.helpText}
      type={field.type}
      name={field.name}
      id={`id_${field.name}`}
      required={field.required}
      maxLength={field.maxLength}
      min={field.min}
      value={values[field.name] ?? ""}
      onChange={(event) => onChange(field.name, event.target.value)}
    />
  ));
}
