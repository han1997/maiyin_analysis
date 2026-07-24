export function DateTimeField({ label, value, onChange, required = false }: { label: string; value: string | null; onChange: (value: string | null) => void; required?: boolean }) {
  return <label className="field"><span>{label}</span><input type="datetime-local" required={required} value={value?.slice(0, 16) ?? ""} onChange={(event) => onChange(event.target.value ? `${event.target.value}:00` : null)} /></label>;
}
