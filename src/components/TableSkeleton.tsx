export function TableSkeleton({ label = "正在加载人员结果" }: { label?: string }) {
  return <div className="table-skeleton" role="status" aria-label={label}>{Array.from({ length: 6 }, (_, index) => <span key={index} />)}</div>;
}
