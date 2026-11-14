interface Option<T extends string> {
  value: T;
  label: string;
}

interface Props<T extends string> {
  value: T;
  options: Option<T>[];
  onChange: (value: T) => void;
}

/** A joined strip of buttons where exactly one option is selected. */
export function Segmented<T extends string>({ value, options, onChange }: Props<T>) {
  return (
    <div className="ds-segmented" role="radiogroup">
      {options.map((o) => (
        <button
          key={o.value}
          type="button"
          role="radio"
          aria-checked={o.value === value}
          className={
            o.value === value
              ? "ds-segmented__option ds-segmented__option--active"
              : "ds-segmented__option"
          }
          onClick={() => onChange(o.value)}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}
