interface Props {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: React.ReactNode;
}

export function Checkbox({ checked, onChange, label }: Props) {
  return (
    <label className="ds-check">
      <input
        type="checkbox"
        checked={checked}
        onChange={(e) => onChange(e.currentTarget.checked)}
      />
      {label}
    </label>
  );
}
