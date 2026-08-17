import type { Instance } from "../../types";
import { InstanceLibraryPage } from "../../pages/InstanceLibraryPage";

export type LibraryPageProps = {
  instances: Instance[];
  onCreate: () => void;
  onPlay: (instance: Instance) => void;
  onClone: (instance: Instance) => void;
  onRename: (instance: Instance) => void;
  onMemoryChange: (instance: Instance, memoryMb: number) => void;
  onRepair: (instance: Instance) => void;
  onDelete: (instance: Instance) => void;
  onOpen: (instance: Instance) => void;
  onOpenDetails: (instance: Instance) => void;
};

export function LibraryPage(props: LibraryPageProps) {
  return (
    <div className="ui3-page-enter">
      <InstanceLibraryPage {...props} />
    </div>
  );
}
