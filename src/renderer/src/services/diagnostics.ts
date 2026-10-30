import type { DiagnosticRecord } from '../../../shared/diagnostics';

export function logDiagnostic(record: DiagnosticRecord): void {
  try { window.windowControls.writeDiagnostic?.(record); } catch {}
}
