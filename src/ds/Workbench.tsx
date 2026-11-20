import type { CSSProperties, ReactNode } from "react";

/** Layout slots only. The caller owns navigation, data and screen state. */
export function Workbench({ navigation, header, children, inspector, footer, className = "" }: {
  navigation?: ReactNode; header?: ReactNode; children: ReactNode;
  inspector?: ReactNode; footer?: ReactNode; className?: string;
}) {
  return <div className={`ds-workbench ${className}`}>
    {navigation && <aside className="ds-workbench__navigation">{navigation}</aside>}
    <main className="ds-workbench__main">{header}{children}{footer}</main>
    {inspector && <aside className="ds-workbench__inspector">{inspector}</aside>}
  </div>;
}

export function PageHeader({ title, leading, metadata, children, actions }: {
  title: ReactNode; leading?: ReactNode; metadata?: ReactNode; children?: ReactNode; actions?: ReactNode;
}) {
  return <header className="ds-page-header">{leading}<h1>{title}</h1>
    {metadata && <span className="ds-page-header__metadata">{metadata}</span>}
    <div className="ds-page-header__spacer" />{children}{actions}
  </header>;
}

export function Pane({ children, footer, className = "", style }: {
  children: ReactNode; footer?: ReactNode; className?: string; style?: CSSProperties;
}) {
  return <section className={`ds-pane ${className}`} style={style}>
    <div className="ds-pane__scroll">{children}</div>
    {footer && <div className="ds-pane__footer">{footer}</div>}
  </section>;
}

export function NavigationGroup({ label, children, action }: { label: string; children: ReactNode; action?: ReactNode }) {
  return <section className="ds-navigation-group"><div className="ds-navigation-group__label"><span>{label}</span>{action}</div>{children}</section>;
}

export function NavigationItem({ label, detail, active = false, onPress, disabled = false }: {
  label: ReactNode; detail?: ReactNode; active?: boolean; onPress: () => void; disabled?: boolean;
}) {
  return <button type="button" className="ds-navigation-item" aria-current={active ? "page" : undefined} onClick={onPress} disabled={disabled}>
    <span>{label}</span>{detail !== undefined && <span className="ds-navigation-item__detail">{detail}</span>}
  </button>;
}

export function PropertyRow({ label, value, onPress }: { label: ReactNode; value: ReactNode; onPress?: () => void }) {
  const content = <><span>{label}</span><span>{value}{onPress && <span className="ds-property-row__arrow"> ›</span>}</span></>;
  return onPress ? <button type="button" className="ds-property-row" onClick={onPress}>{content}</button>
    : <div className="ds-property-row">{content}</div>;
}
