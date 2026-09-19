import { useCallback, useSyncExternalStore } from "react";
import {
  RIGHT_SECTIONS_DEFAULTS,
  loadRightSections,
  setRightSection,
  subscribeRightSections,
  type RightSectionId,
} from "./layoutStore";

export function useRightSection(id: RightSectionId): [boolean, () => void] {
  const open = useSyncExternalStore(
    subscribeRightSections,
    () => loadRightSections()[id],
    () => RIGHT_SECTIONS_DEFAULTS[id],
  );

  const toggle = useCallback((): void => {
    setRightSection(id, !loadRightSections()[id]);
  }, [id]);

  return [open, toggle];
}
