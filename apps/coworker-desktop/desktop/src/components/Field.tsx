import type { ReactNode } from "react";
import { CheckCircle2, Circle, XCircle, AlertCircle } from "lucide-react";
import type { FeedbackTone } from "../lib/bridgeLogic";

export function Field({
  label,
  inputId,
  error,
  children,
  className,
}: {
  label: string;
  inputId: string;
  error?: string;
  children: ReactNode;
  className?: string;
}) {
  return (
    <label className={className ? `field ${className}` : "field"} htmlFor={inputId}>
      <span>{label}</span>
      {children}
      {error && <small role="alert">{error}</small>}
    </label>
  );
}

export function FeedbackIcon({ tone }: { tone: FeedbackTone }) {
  if (tone === "success") return <CheckCircle2 size={16} aria-hidden="true" />;
  if (tone === "error") return <XCircle size={16} aria-hidden="true" />;
  if (tone === "warning") return <AlertCircle size={16} aria-hidden="true" />;
  return <Circle size={12} aria-hidden="true" />;
}
