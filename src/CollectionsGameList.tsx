import type { KeyboardEvent as ReactKeyboardEvent, ReactNode } from "react";
import { Collapse, staggerStyle } from "./motion";
import type { SavedCollectionGameGroup } from "./savedCollectionGroups";
import type { SavedCollectionEntry } from "./types";

export type CollectionsGameListProps = {
  groups: SavedCollectionGameGroup[];
  expandedGameKey: string | null;
  onToggleGameKey: (key: string) => void;
  groupLimit?: number;
  expandedGroupLimits?: Record<string, number>;
  onShowMoreChildren?: (groupKey: string) => void;
  renderPoster: (coverPath: string | null) => ReactNode;
  renderChild: (
    group: SavedCollectionGameGroup,
    entry: SavedCollectionEntry,
    childIndex: number,
  ) => ReactNode;
  emptyMessage: string;
};

export function CollectionsGameList({
  groups,
  expandedGameKey,
  onToggleGameKey,
  groupLimit,
  expandedGroupLimits,
  onShowMoreChildren,
  renderPoster,
  renderChild,
  emptyMessage,
}: CollectionsGameListProps) {
  const visibleGroups =
    groupLimit != null ? groups.slice(0, groupLimit) : groups;

  return (
    <div className="results-fill">
      <ul className="list collections-by-game">
        {visibleGroups.map((group, idx) => {
          const expanded = expandedGameKey === group.key;
          const childLimit = expandedGroupLimits?.[group.key] ?? 25;
          const visibleChildren = expanded
            ? group.collections.slice(0, childLimit)
            : [];
          return (
            <li
              key={group.key}
              className="collections-game-row list-row-poster stagger-item"
              style={staggerStyle(idx)}
            >
              <div
                className="collections-game-header list-row-clickable"
                aria-expanded={expanded}
                onClick={() => onToggleGameKey(group.key)}
                onKeyDown={(e: ReactKeyboardEvent<HTMLDivElement>) => {
                  if (e.key === "Enter" || e.key === " ") {
                    e.preventDefault();
                    onToggleGameKey(group.key);
                  }
                }}
                role="button"
                tabIndex={0}
              >
                {renderPoster(group.coverPath)}
                <div className="list-row-main">
                  <strong>{group.title}</strong>
                  <small>
                    {group.collections.length} collection
                    {group.collections.length === 1 ? "" : "s"}
                    {group.managed ? " · managed" : " · not managed"}
                  </small>
                </div>
              </div>
              <Collapse show={expanded} className="collections-game-expand">
                <ul className="list collections-game-children">
                  {visibleChildren.map((c, childIdx) =>
                    renderChild(group, c, childIdx),
                  )}
                </ul>
                {expanded &&
                  onShowMoreChildren &&
                  group.collections.length > visibleChildren.length && (
                    <div className="list-load-more">
                      <button
                        type="button"
                        className="linkish"
                        onClick={() => onShowMoreChildren(group.key)}
                      >
                        Show more (
                        {group.collections.length - visibleChildren.length}{" "}
                        remaining)
                      </button>
                    </div>
                  )}
              </Collapse>
            </li>
          );
        })}
        {groups.length === 0 && <li className="empty">{emptyMessage}</li>}
      </ul>
    </div>
  );
}
