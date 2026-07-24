import { Icon } from "./Icon";

export function ConfirmDialog({ title, description, confirmLabel, onCancel, onConfirm }: { title: string; description: string; confirmLabel: string; onCancel: () => void; onConfirm: () => void }) {
  return <div className="panel-backdrop confirm-backdrop"><section className="confirm-dialog" role="alertdialog" aria-modal="true" aria-labelledby="confirm-title"><span className="confirm-icon"><Icon name="trash" /></span><h2 id="confirm-title">{title}</h2><p>{description}</p><div><button className="button button-quiet" type="button" onClick={onCancel}>取消</button><button className="button button-danger" type="button" onClick={onConfirm}>{confirmLabel}</button></div></section></div>;
}
