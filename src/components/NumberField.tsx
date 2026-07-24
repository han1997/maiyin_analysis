export function NumberField({ label, value, onChange, required = false }: { label: string; value: number | null; onChange: (value: number | null) => void; required?: boolean }) {
  return <label className="field"><span>{label}</span><input type="number" min="0" required={required} value={value ?? ""} placeholder="不限" onChange={(event) => onChange(event.target.value === "" ? null : Number(event.target.value))} /></label>;
}
