/**
 * A styled native `<select>`. Native gives us keyboard navigation, accessibility,
 * and OS-consistent popups for free — and stays compact as the option count
 * grows (unlike a stacked radio list). Options are `{ value, label }`; the
 * caller renders any per-option description separately (a `<select>` can't).
 */

export interface SelectOption {
  value: string;
  label: string;
}

interface Props {
  value: string;
  options: SelectOption[];
  onChange: (value: string) => void;
  className?: string;
  disabled?: boolean;
}

export function Select({ value, options, onChange, className = "", disabled }: Props) {
  return (
    <select
      className={`ds-select ${className}`}
      value={value}
      disabled={disabled}
      onChange={(e) => onChange(e.currentTarget.value)}
    >
      {options.map((opt) => (
        <option key={opt.value} value={opt.value}>
          {opt.label}
        </option>
      ))}
    </select>
  );
}
