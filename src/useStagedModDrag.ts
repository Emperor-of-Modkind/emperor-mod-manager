import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
} from "react";
import { stagedModGroup, type StagedModGroup } from "./stagedLoadOrder";
import type { StagedMod } from "./types";

const LONG_PRESS_MS = 350;
const MOVE_CANCEL_PX = 8;
const EDGE_SCROLL_PX = 56;
const EDGE_SCROLL_SPEED = 14;

export type StagedModDragLift = {
  x: number;
  y: number;
  width: number;
  height: number;
};

type DragSession = {
  modId: string;
  group: StagedModGroup;
  startGroupIndex: number;
  pointerId: number;
  startX: number;
  startY: number;
  offsetY: number;
  activated: boolean;
};

/**
 * Where to insert `modId` inside its accordion group, based on the list's box.
 * `display` is the on-screen order. The result is an index among the other
 * rows in that group, or that group's length when the pointer is past the end.
 * Unmounted rows still count: each slot is an equal share of the list height.
 */
export function stagedDropGroupIndex(
  display: readonly { id: string; enabled: boolean; plugin_tool_id?: string | null }[],
  modId: string,
  group: StagedModGroup,
  pointerY: number,
  listTop: number,
  listHeight: number,
): number {
  const others = display.filter(
    (m) => stagedModGroup(m) === group && m.id !== modId,
  );
  if (others.length === 0 || !(listHeight > 0) || display.length === 0) {
    return others.length;
  }
  const stride = listHeight / display.length;
  const y = pointerY - listTop;
  let insertAt = 0;
  for (let i = 0; i < display.length; i++) {
    const item = display[i];
    if (stagedModGroup(item) !== group || item.id === modId) continue;
    if (y < i * stride + stride / 2) return insertAt;
    insertAt += 1;
  }
  return insertAt;
}

function groupIndexOf(mods: StagedMod[], modId: string): number {
  const mod = mods.find((m) => m.id === modId);
  if (!mod) return -1;
  const group = stagedModGroup(mod);
  return mods
    .filter((m) => stagedModGroup(m) === group)
    .findIndex((m) => m.id === modId);
}

function findScrollParent(el: HTMLElement | null): HTMLElement | null {
  let node: HTMLElement | null = el;
  while (node) {
    const { overflowY } = getComputedStyle(node);
    if (
      (overflowY === "auto" || overflowY === "scroll" || overflowY === "overlay") &&
      node.scrollHeight > node.clientHeight
    ) {
      return node;
    }
    node = node.parentElement;
  }
  return null;
}

/** Preview display order while dragging within the tools, enabled, or disabled block. */
export function previewVisibleMods(
  visibleMods: StagedMod[],
  draggingId: string | null,
  previewGroupIndex: number | null,
): StagedMod[] {
  if (!draggingId || previewGroupIndex == null) return visibleMods;
  const dragged = visibleMods.find((m) => m.id === draggingId);
  if (!dragged) return visibleMods;
  const groupKey = stagedModGroup(dragged);
  const tools = visibleMods.filter((m) => stagedModGroup(m) === "tools");
  const enabled = visibleMods.filter((m) => stagedModGroup(m) === "enabled");
  const disabled = visibleMods.filter((m) => stagedModGroup(m) === "disabled");
  const group =
    groupKey === "tools"
      ? [...tools]
      : groupKey === "enabled"
        ? [...enabled]
        : [...disabled];
  const from = group.findIndex((m) => m.id === draggingId);
  if (from < 0) return visibleMods;
  const clamped = Math.max(0, Math.min(group.length - 1, previewGroupIndex));
  const nextGroup = [...group];
  const [moved] = nextGroup.splice(from, 1);
  nextGroup.splice(clamped, 0, moved);
  if (groupKey === "tools") return [...nextGroup, ...enabled, ...disabled];
  if (groupKey === "enabled") return [...tools, ...nextGroup, ...disabled];
  return [...tools, ...enabled, ...nextGroup];
}

type UseStagedModDragArgs = {
  canReorder: boolean;
  visibleMods: StagedMod[];
  onCommit: (modId: string, targetGroupIndex: number) => void | Promise<void>;
};

export function useStagedModDrag({
  canReorder,
  visibleMods,
  onCommit,
}: UseStagedModDragArgs) {
  const [draggingId, setDraggingId] = useState<string | null>(null);
  const [previewGroupIndex, setPreviewGroupIndex] = useState<number | null>(
    null,
  );
  const [lift, setLift] = useState<StagedModDragLift | null>(null);
  const [suppressClick, setSuppressClick] = useState(false);

  const sessionRef = useRef<DragSession | null>(null);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const rowRefs = useRef(new Map<string, HTMLElement>());
  const toolsListRef = useRef<HTMLUListElement | null>(null);
  const listRef = useRef<HTMLUListElement | null>(null);
  const disabledListRef = useRef<HTMLUListElement | null>(null);
  const visibleModsRef = useRef(visibleMods);
  const onCommitRef = useRef(onCommit);
  const previewRef = useRef<number | null>(null);

  visibleModsRef.current = visibleMods;
  onCommitRef.current = onCommit;
  previewRef.current = previewGroupIndex;

  const activeListFor = useCallback((group: StagedModGroup) => {
    if (group === "tools") return toolsListRef.current ?? listRef.current;
    if (group === "disabled") return disabledListRef.current ?? listRef.current;
    return listRef.current;
  }, []);

  const clearTimer = useCallback(() => {
    if (timerRef.current != null) {
      clearTimeout(timerRef.current);
      timerRef.current = null;
    }
  }, []);

  const setRowRef = useCallback((modId: string, el: HTMLElement | null) => {
    if (el) rowRefs.current.set(modId, el);
    else rowRefs.current.delete(modId);
  }, []);

  const clearListDragStyle = useCallback(() => {
    for (const el of [toolsListRef.current, listRef.current, disabledListRef.current]) {
      if (el) {
        el.style.touchAction = "";
        el.style.userSelect = "";
      }
    }
  }, []);

  const endDrag = useCallback(() => {
    clearTimer();
    clearListDragStyle();
    sessionRef.current = null;
    setDraggingId(null);
    setPreviewGroupIndex(null);
    setLift(null);
  }, [clearListDragStyle, clearTimer]);

  const activateDrag = useCallback(
    (session: DragSession, target: HTMLElement) => {
      const row = target.closest("li");
      if (!(row instanceof HTMLElement)) return;
      const rect = row.getBoundingClientRect();
      session.activated = true;
      session.offsetY = session.startY - rect.top;
      // Capture on the list so the row can unmount into a placeholder + floating clone.
      const activeList = activeListFor(session.group);
      const captureEl = activeList ?? target;
      if (activeList) {
        activeList.style.touchAction = "none";
        activeList.style.userSelect = "none";
      }
      try {
        captureEl.setPointerCapture(session.pointerId);
      } catch {
        /* already released */
      }
      setDraggingId(session.modId);
      setPreviewGroupIndex(session.startGroupIndex);
      previewRef.current = session.startGroupIndex;
      setLift({
        x: rect.left,
        y: rect.top,
        width: rect.width,
        height: rect.height,
      });
      setSuppressClick(true);
    },
    [activeListFor],
  );

  const indexFromPointerY = useCallback(
    (pointerY: number, modId: string, group: StagedModGroup) => {
      const list = activeListFor(group);
      const display = previewVisibleMods(
        visibleModsRef.current,
        modId,
        previewRef.current,
      );
      if (!list) {
        return display.filter((m) => stagedModGroup(m) === group && m.id !== modId)
          .length;
      }
      const splitList =
        toolsListRef.current != null ||
        disabledListRef.current != null ||
        (group === "enabled" &&
          listRef.current != null &&
          display.some((m) => stagedModGroup(m) !== "enabled"));
      const scopedDisplay = splitList
        ? display.filter((m) => stagedModGroup(m) === group)
        : display;
      const rect = list.getBoundingClientRect();
      return stagedDropGroupIndex(
        scopedDisplay,
        modId,
        group,
        pointerY,
        rect.top,
        rect.height,
      );
    },
    [activeListFor],
  );

  const onPointerMove = useCallback(
    (event: PointerEvent) => {
      const session = sessionRef.current;
      if (!session || event.pointerId !== session.pointerId) return;

      if (!session.activated) {
        const dx = event.clientX - session.startX;
        const dy = event.clientY - session.startY;
        if (dx * dx + dy * dy > MOVE_CANCEL_PX * MOVE_CANCEL_PX) {
          clearTimer();
          sessionRef.current = null;
        }
        return;
      }

      event.preventDefault();
      setLift((prev) =>
        prev
          ? { ...prev, y: event.clientY - session.offsetY }
          : prev,
      );

      const nextIndex = indexFromPointerY(
        event.clientY,
        session.modId,
        session.group,
      );
      if (nextIndex !== previewRef.current) {
        previewRef.current = nextIndex;
        setPreviewGroupIndex(nextIndex);
      }

      const scrollParent = findScrollParent(activeListFor(session.group));
      if (scrollParent) {
        const bounds = scrollParent.getBoundingClientRect();
        if (event.clientY < bounds.top + EDGE_SCROLL_PX) {
          scrollParent.scrollTop -= EDGE_SCROLL_SPEED;
        } else if (event.clientY > bounds.bottom - EDGE_SCROLL_PX) {
          scrollParent.scrollTop += EDGE_SCROLL_SPEED;
        }
      }
    },
    [activeListFor, clearTimer, indexFromPointerY],
  );

  const onPointerUp = useCallback(
    (event: PointerEvent) => {
      const session = sessionRef.current;
      if (!session || event.pointerId !== session.pointerId) return;

      clearTimer();
      clearListDragStyle();
      const activated = session.activated;
      const modId = session.modId;
      const startIndex = session.startGroupIndex;
      const targetIndex = previewRef.current;
      sessionRef.current = null;
      setDraggingId(null);
      setPreviewGroupIndex(null);
      setLift(null);

      if (
        activated &&
        targetIndex != null &&
        targetIndex !== startIndex
      ) {
        void onCommitRef.current(modId, targetIndex);
      }

      if (activated) {
        // Keep suppressClick through the synthetic click that follows pointerup.
        window.setTimeout(() => setSuppressClick(false), 0);
      }
    },
    [clearListDragStyle, clearTimer],
  );

  useEffect(() => {
    window.addEventListener("pointermove", onPointerMove);
    window.addEventListener("pointerup", onPointerUp);
    window.addEventListener("pointercancel", onPointerUp);
    return () => {
      window.removeEventListener("pointermove", onPointerMove);
      window.removeEventListener("pointerup", onPointerUp);
      window.removeEventListener("pointercancel", onPointerUp);
    };
  }, [onPointerMove, onPointerUp]);

  useEffect(() => {
    if (!canReorder && sessionRef.current) endDrag();
  }, [canReorder, endDrag]);

  useEffect(() => () => clearTimer(), [clearTimer]);

  const onDragPointerDown = useCallback(
    (modId: string, event: ReactPointerEvent) => {
      if (!canReorder) return;
      if (event.button !== 0) return;
      const target = event.target;
      if (!(target instanceof Element)) return;
      if (
        target.closest(
          "button, input, a, label.check, .actions, .mod-collection-chip",
        )
      ) {
        return;
      }

      const startGroupIndex = groupIndexOf(visibleModsRef.current, modId);
      if (startGroupIndex < 0) return;
      const mod = visibleModsRef.current.find((m) => m.id === modId);
      if (!mod) return;

      clearTimer();
      const session: DragSession = {
        modId,
        group: stagedModGroup(mod),
        startGroupIndex,
        pointerId: event.pointerId,
        startX: event.clientX,
        startY: event.clientY,
        offsetY: 0,
        activated: false,
      };
      sessionRef.current = session;
      const el = event.currentTarget;
      if (!(el instanceof HTMLElement)) return;
      timerRef.current = setTimeout(() => {
        timerRef.current = null;
        if (sessionRef.current === session) activateDrag(session, el);
      }, LONG_PRESS_MS);
    },
    [activateDrag, canReorder, clearTimer],
  );

  const displayMods = useMemo(
    () => previewVisibleMods(visibleMods, draggingId, previewGroupIndex),
    [visibleMods, draggingId, previewGroupIndex],
  );

  const toolsDisplayMods = useMemo(
    () => displayMods.filter((m) => stagedModGroup(m) === "tools"),
    [displayMods],
  );

  const enabledDisplayMods = useMemo(
    () => displayMods.filter((m) => stagedModGroup(m) === "enabled"),
    [displayMods],
  );

  const disabledDisplayMods = useMemo(
    () => displayMods.filter((m) => stagedModGroup(m) === "disabled"),
    [displayMods],
  );

  return {
    toolsListRef,
    listRef,
    disabledListRef,
    setRowRef,
    displayMods,
    toolsDisplayMods,
    enabledDisplayMods,
    disabledDisplayMods,
    draggingId,
    previewGroupIndex,
    lift,
    suppressClick,
    onDragPointerDown,
    isDragging: draggingId != null,
  };
}
