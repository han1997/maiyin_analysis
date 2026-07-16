export function formatInteger(value: number): string {
  return new Intl.NumberFormat("zh-CN").format(value);
}

export function formatDateTime(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return new Intl.DateTimeFormat("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  }).format(date);
}

export function maskIdentity(value: string): string {
  if (value.length < 10) return value;
  return `${value.slice(0, 6)}••••••${value.slice(-4)}`;
}

export function maskPhone(value: string): string {
  if (value.length < 7) return value;
  return `${value.slice(0, 3)}••••${value.slice(-4)}`;
}

export function joinScope(parts: string[]): string {
  const active = parts.filter(Boolean);
  return active.length ? active.join(" / ") : "全部辖区";
}

