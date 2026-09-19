import { useCallback, useState } from "react";
import { loadRightSections, setRightSection, type RightSectionId } from "./layoutStore";

export function useRightSection(id: RightSectionId): [boolean, () => void] {
  const [open, setOpen] = useState<boolean>(() => loadRightSections()[id]);

  const toggle = useCallback((): void => {
    setOpen((prev) => {
      const next = !prev;
      setRightSection(id, next);
      return next;
    });
  }, [id]);

  return [open, toggle];
}
