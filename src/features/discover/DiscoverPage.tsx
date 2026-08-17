import type {
  BackupItem,
  ContentItem,
  Instance,
  JavaRuntime,
  ModInspection,
  ModUpdateInfo,
  ModpackArchive,
  ModpackInspection,
  OnlineProject,
} from "../../types";
import {
  ArchiveContentPage,
  ModpacksPage,
  ModsPage,
} from "../../pages/ContentPages";
import { Tabs } from "../../ui/components";
import { DISCOVER_TABS } from "../../app/Router";
import type { DiscoverTab } from "../../app/Router";
import type { ContentKind } from "../instance/InstancePage";

export type DiscoverPageProps = {
  tab: DiscoverTab;
  onTab: (tab: DiscoverTab) => void;
  onContentKindChange: (kind: ContentKind) => void;
  instances: Instance[];
  targetId?: number;
  onTarget: (id: number) => void;
  busy: boolean;
  message: string;
  dragging: boolean;
  // Mods
  modItems: ContentItem[];
  modInspection?: ModInspection;
  modQueueCount: number;
  onlineModQuery: string;
  onlineModProjects: OnlineProject[];
  modLoader: string;
  modVersion: string;
  problemMods: Record<string, string>;
  modUpdates: ModUpdateInfo[];
  removedBackups: BackupItem[];
  onPickMod: () => void;
  onInstallMod: () => void;
  onToggleMod: (item: ContentItem) => void;
  onRemoveMod: (item: ContentItem) => void;
  onOnlineModQuery: (value: string) => void;
  onOnlineModSearch: () => void;
  onOnlineModInstall: (project: OnlineProject) => void;
  onTranslate: (text: string) => Promise<string | undefined>;
  onInstallCurseforgeUrl: (url: string) => void;
  onOnlineModLoader: (value: string) => void;
  onOnlineModVersion: (value: string) => void;
  onCheckModUpdates: () => void;
  onUpdateMod: (item: ContentItem) => void;
  onUpdateAllMods: () => void;
  onRestoreBackup: (item: BackupItem) => void;
  onOpenModFolder: () => void;
  // Modpacks
  packInspection?: ModpackInspection;
  onlinePackQuery: string;
  onlinePackProjects: OnlineProject[];
  archives: ModpackArchive[];
  javaRuntimes: JavaRuntime[];
  onPickPack: () => void;
  onImportPack: (gameVersion?: string, loaderType?: string) => void;
  onOnlinePackQuery: (value: string) => void;
  onOnlinePackSearch: () => void;
  onOnlinePackInstall: (project: OnlineProject) => void;
  onExportPack: (instanceId: number, includeSaves: boolean) => void;
  onImportArchiveInstance: (archive: ModpackArchive) => void;
  onRemoveArchive: (archive: ModpackArchive) => void;
  onInstallJava: (major: number) => void;
  // Resource packs / shaders
  archiveItems: ContentItem[];
  onToggleArchive: (item: ContentItem) => void;
  onRemoveArchiveItem: (item: ContentItem) => void;
  onImportArchive: (kind: "resourcepack" | "shaderpack") => void;
  onOpenArchiveFolder: (kind: "resourcepacks" | "shaderpacks") => void;
};

export function DiscoverPage(props: DiscoverPageProps) {
  const { tab, onTab } = props;
  const changeTab = (next: DiscoverTab) => {
    onTab(next);
    props.onContentKindChange(
      next === "mods"
        ? "mod"
        : next === "resourcepacks"
          ? "resourcepack"
          : next === "shaders"
            ? "shaderpack"
            : undefined,
    );
  };
  return (
    <div className="ui3-page-enter">
      <header className="ui3-page-header">
        <div>
          <h1>发现</h1>
          <p>从 Modrinth 在线发现模组、整合包、资源包与光影。</p>
        </div>
      </header>
      <Tabs
        tabs={DISCOVER_TABS}
        value={tab}
        onChange={changeTab}
        label="发现分类"
      />
      {tab === "mods" ? (
        <ModsPage
          instances={props.instances}
          selectedId={props.targetId}
          onSelect={props.onTarget}
          items={props.modItems}
          inspection={props.modInspection}
          busy={props.busy}
          message={props.message}
          onPick={props.onPickMod}
          onInstall={props.onInstallMod}
          onToggle={props.onToggleMod}
          onRemove={props.onRemoveMod}
          queuedCount={props.modQueueCount}
          dragging={props.dragging}
          onlineQuery={props.onlineModQuery}
          onlineProjects={props.onlineModProjects}
          onOnlineQuery={props.onOnlineModQuery}
          onOnlineSearch={props.onOnlineModSearch}
          onOnlineInstall={props.onOnlineModInstall}
          onTranslate={props.onTranslate}
          onInstallCurseforgeUrl={props.onInstallCurseforgeUrl}
          onlineLoader={props.modLoader}
          onlineVersion={props.modVersion}
          onOnlineLoader={props.onOnlineModLoader}
          onOnlineVersion={props.onOnlineModVersion}
          problemMods={props.problemMods}
          updates={props.modUpdates}
          onCheckUpdates={props.onCheckModUpdates}
          onUpdate={props.onUpdateMod}
          onUpdateAll={props.onUpdateAllMods}
          backups={props.removedBackups}
          onRestore={props.onRestoreBackup}
          onOpenFolder={props.onOpenModFolder}
        />
      ) : null}
      {tab === "modpacks" ? (
        <ModpacksPage
          inspection={props.packInspection}
          busy={props.busy}
          message={props.message}
          dragging={props.dragging}
          onPick={props.onPickPack}
          onImport={props.onImportPack}
          instances={props.instances}
          targetId={props.targetId}
          onTarget={props.onTarget}
          onlineQuery={props.onlinePackQuery}
          onlineProjects={props.onlinePackProjects}
          onOnlineQuery={props.onOnlinePackQuery}
          onOnlineSearch={props.onOnlinePackSearch}
          onOnlineInstall={props.onOnlinePackInstall}
          onTranslate={props.onTranslate}
          onExport={props.onExportPack}
          archives={props.archives}
          javaRuntimes={props.javaRuntimes}
          onImportArchive={props.onImportArchiveInstance}
          onRemoveArchive={props.onRemoveArchive}
          onInstallJava={props.onInstallJava}
        />
      ) : null}
      {tab === "resourcepacks" || tab === "shaders" ? (
        <ArchiveContentPage
          title={tab === "resourcepacks" ? "资源包" : "光影"}
          kind={tab === "resourcepacks" ? "resourcepack" : "shaderpack"}
          instances={props.instances}
          targetId={props.targetId}
          items={props.archiveItems}
          busy={props.busy}
          message={props.message}
          dragging={props.dragging}
          onTarget={props.onTarget}
          onImport={() =>
            props.onImportArchive(
              tab === "resourcepacks" ? "resourcepack" : "shaderpack",
            )
          }
          onToggle={props.onToggleArchive}
          onRemove={props.onRemoveArchiveItem}
          backups={props.removedBackups}
          onRestore={props.onRestoreBackup}
          onOpenFolder={() =>
            props.onOpenArchiveFolder(
              tab === "resourcepacks" ? "resourcepacks" : "shaderpacks",
            )
          }
        />
      ) : null}
    </div>
  );
}
