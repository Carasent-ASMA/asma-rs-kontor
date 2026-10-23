import { nativeDesk, type OrganizationGroup, type OrganizationGroupKind } from "./session-organization";

export const KIND_ICONS: Partial<Record<OrganizationGroupKind, string>> = { workstream: "Route", epic: "Flag", workspace: "PanelsTopLeft", project: "Folder" };

/** Presentation only: a glyph never establishes desk membership or durable configuration. */
export function deskIcon(key: string): string {
  switch (key.trim().toLowerCase().replace(/[\s_]+/g, "-")) {
    case "pr-review": return "GitPullRequest";
    case "adam": return "Bot";
    case "qa": return "FlaskConical";
    case "qsw": return "Zap";
    default: return "Armchair";
  }
}

export function organizationIcon(group: OrganizationGroup): string | undefined {
  if (group.desk) return deskIcon(nativeDesk(group.project?.name) ?? group.label);
  if (group.kind === "workspace" && group.parentKey?.includes("/kind:")) return undefined;
  return KIND_ICONS[group.kind];
}
