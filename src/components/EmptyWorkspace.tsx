import { Icon } from "./Icon";

export function EmptyWorkspace({ onFiles, onFolder }: { onFiles: () => void; onFolder: () => void }) {
  return <section className="empty-workspace"><div className="empty-illustration" aria-hidden="true"><Icon name="file" size={38}/><span><Icon name="search" size={20}/></span></div><span className="empty-kicker">第一步</span><h2>导入入住数据</h2><p>选择 Excel、CSV 文件或整个文件夹。导入后会自动清洗记录、计算风险并保留核查证据。</p><div><button className="button button-primary" type="button" onClick={onFiles}><Icon name="upload"/>选择文件</button><button className="button button-secondary" type="button" onClick={onFolder}><Icon name="folder"/>选择文件夹</button></div><small><Icon name="shield" size={15}/> 全程在本机处理，不上传文件</small></section>;
}
