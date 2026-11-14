import type { InputHTMLAttributes, ReactNode, SelectHTMLAttributes, TextareaHTMLAttributes } from "react";

/** Upstream's `tailwind_formfield` widget classes: red when the field has errors. */
export function widgetClass(errors?: string[]) {
  return `${errors?.length ? "bg-red-50 dark:bg-red-900" : "bg-slate-50 dark:bg-slate-800"} pl-4 py-2 md:py-4 focus:outline-none w-full`;
}

type FieldProps = {
  label: string;
  errors?: string[] | undefined;
  helpText?: ReactNode;
  /** `tailwind_formfield_implicit`: no label above, the label as the placeholder. */
  implicit?: boolean;
  children: ReactNode;
};

/** Upstream's `tailwind_forms/formfield.html`: label, widget, then the errors or else the help text. */
export function FormField({ label, errors, helpText, implicit, children }: FieldProps) {
  return (
    <div className="text-lg mb-6 md:mb-8">
      {!implicit && <div className="text-slate-800 dark:text-slate-100 font-bold">{label}:</div>}
      {children}
      {errors?.length
        ? errors.map((error) => (
            <div key={error} className="text-red-500 dark:text-red-400 pt-1 px-2 text-sm">
              {error}
            </div>
          ))
        : helpText && <div className="text-gray-500 dark:text-gray-400 pt-1 px-2 text-sm">{helpText}</div>}
    </div>
  );
}

type InputFieldProps = Omit<FieldProps, "children"> & InputHTMLAttributes<HTMLInputElement>;

export function InputField({ label, errors, helpText, implicit, ...input }: InputFieldProps) {
  return (
    <FormField label={label} errors={errors} helpText={helpText} implicit={implicit ?? false}>
      <input {...input} placeholder={implicit ? label : input.placeholder} className={widgetClass(errors)} />
    </FormField>
  );
}

type SelectFieldProps = Omit<FieldProps, "children"> & SelectHTMLAttributes<HTMLSelectElement>;

export function SelectField({
  label,
  errors,
  helpText,
  children,
  ...select
}: SelectFieldProps & { children: ReactNode }) {
  return (
    <FormField label={label} errors={errors} helpText={helpText}>
      <select {...select} className={widgetClass(errors)}>
        {children}
      </select>
    </FormField>
  );
}

type TextareaFieldProps = Omit<FieldProps, "children"> & TextareaHTMLAttributes<HTMLTextAreaElement>;

export function TextareaField({ label, errors, helpText, ...textarea }: TextareaFieldProps) {
  return (
    <FormField label={label} errors={errors} helpText={helpText}>
      <textarea {...textarea} className={widgetClass(errors)} />
    </FormField>
  );
}

/** Errors of the form as a whole (Django's `non_field_errors`, `__all__`). */
export function FormErrors({ errors }: { errors?: string[] | undefined }) {
  return errors?.map((error) => (
    <div key={error} className="text-red-500 dark:text-red-400 pt-1 px-2 text-sm mb-6">
      {error}
    </div>
  ));
}
