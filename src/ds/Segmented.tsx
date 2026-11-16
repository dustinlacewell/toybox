interface Option<T extends string> {
  value: T;
  label: string;
}

interface Props<T extends string> {
  label?: string;
  value: T;
  options: Option<T>[];
  onChange: (value: T) => void;
}

/** A joined strip of buttons where exactly one option is selected. */
export function Segmented<T extends string>({ value, options, onChange, label }: Props<T>) {
  return (
    <div className="ds-segmented" role="radiogroup" aria-label={label} onKeyDown={(event) => {
      const direction = event.key === "ArrowRight" || event.key === "ArrowDown" ? 1 : event.key === "ArrowLeft" || event.key === "ArrowUp" ? -1 : 0;
      if (!direction || !options.length) return;
      event.preventDefault();
      const next = (Math.max(0, options.findIndex((o) => o.value === value)) + direction + options.length) % options.length;
      onChange(options[next].value);
      event.currentTarget.querySelectorAll("button")[next]?.focus();
    }}>
      {options.map((o) => (
        <button
          key={o.value}
          type="button"
          role="radio"
          aria-checked={o.value === value}
          tabIndex={o.value === value ? 0 : -1}
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
