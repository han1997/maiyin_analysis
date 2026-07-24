const pageSizeOptions = [50, 100, 200] as const;

export function PageSizeSelect({ label, unit, value, onChange }: { label: string; unit: string; value: number; onChange: (value: number) => void }) {
  return (
    <label className="page-size-control">
      <span>每页</span>
      <select aria-label={label} value={value} onChange={(event) => onChange(Number(event.target.value))}>
        {pageSizeOptions.map((option) => <option key={option} value={option}>{option}</option>)}
      </select>
      <span>{unit}</span>
    </label>
  );
}
