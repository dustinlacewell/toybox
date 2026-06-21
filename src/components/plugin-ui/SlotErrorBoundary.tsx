/**
 * Error boundary around one plugin slot component. A throwing plugin renders the
 * host-supplied `fallback` (declarative fields, or nothing) instead of taking
 * down the drawer, and each throw is recorded toward the session disable limit.
 */

import { Component, type ErrorInfo, type ReactNode } from "react";

import { recordSlotThrow } from "./slotDisableRegistry";

interface Props {
  pluginId: string;
  moduleRel: string;
  fallback: ReactNode;
  children: ReactNode;
}

interface State {
  failed: boolean;
}

export class SlotErrorBoundary extends Component<Props, State> {
  state: State = { failed: false };

  static getDerivedStateFromError(): State {
    return { failed: true };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    const { pluginId, moduleRel } = this.props;
    const disabled = recordSlotThrow(pluginId, moduleRel);
    console.error(
      `[plugins] slot '${pluginId}/${moduleRel}' threw${disabled ? " (now disabled for session)" : ""}:`,
      error,
      info.componentStack,
    );
  }

  render() {
    return this.state.failed ? this.props.fallback : this.props.children;
  }
}
