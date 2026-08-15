import { X } from "lucide-react";

type IncompatibleGroup = {
  instanceId: number;
  instanceName: string;
  mods: Array<{ fileName: string; reason: string }>;
};

export function IncompatibleModsModal({
  groups,
  onDelete,
  onClose,
}: {
  groups: IncompatibleGroup[];
  onDelete: () => void;
  onClose: () => void;
}) {
  const total = groups.reduce((sum, group) => sum + group.mods.length, 0);
  return (
    <div
      className="update-modal-backdrop"
      role="alertdialog"
      aria-modal="true"
      aria-label="不兼容模组"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <div className="changelog-modal error-modal">
        <button
          className="update-modal-close"
          type="button"
          aria-label="关闭"
          onClick={onClose}
        >
          <X size={18} />
        </button>
        <h2>检测到 {total} 个不兼容模组</h2>
        <div className="error-modal-body">
          {groups.map((group) => (
            <div key={group.instanceId}>
              <p>
                <b>{group.instanceName}</b>
              </p>
              <ul className="incompatible-mod-list">
                {group.mods.map((mod, index) => (
                  <li key={index}>
                    {mod.fileName} —— {mod.reason}
                  </li>
                ))}
              </ul>
            </div>
          ))}
          <p>
            这些模组与当前加载器/游戏版本不匹配，可能导致游戏启动失败。建议删除
            （删除前会先移到该实例的可恢复备份区）。
          </p>
        </div>
        <div className="error-modal-actions">
          <button className="primary" type="button" onClick={onDelete}>
            删除不兼容模组
          </button>
          <button type="button" onClick={onClose}>
            暂不处理
          </button>
        </div>
      </div>
    </div>
  );
}
