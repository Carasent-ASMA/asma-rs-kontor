import type { OrganizationGroup } from "./session-organization";

/**
 * Presentation over a built hierarchy tree; membership, keys and counts stay as built. A native project whose
 * sessions settle different epics shows one row per epic, so its Epic unknown row is placed and worded beside the
 * same project's recognised rows, and sibling rows that different native projects label alike get a folder name.
 * Projects match by native ID only, never by title; nothing here assigns an epic or a workstream.
 */
export function relateSplitProjects(tree: readonly OrganizationGroup[], roots: ReadonlyMap<string, string> = new Map()): OrganizationGroup[] {
  const places = tree.flatMap((top) => top.kind !== "workstream" ? [] : top.groups.flatMap((group) => {
    const project = group.kind === "epic" ? projectOf(group) : null;
    return project ? [{ group, stream: top.label, project }] : [];
  }));
  const details = new Map<string, string>();
  for (const { group, stream, project } of places) {
    const others = places.filter((other) => other.project === project && (other.group.epic === null) !== (group.epic === null));
    if (group.epic === null) {
      // Keeps a detail the tree already gives, in the same words it uses for workspaces without sessions.
      const related = others.length ? `same project as ${unique(others.map((other) => other.stream === stream ? other.group.epic! : `${other.group.epic} in ${other.stream}`)).join(", ")}` : null;
      details.set(group.key, [group.detail ?? contents(group), related].filter(Boolean).join(" · "));
    } else if (others.length) {
      // Counts the project's sessions with no known epic or, when it has none, its workspaces without sessions, and
      // says which workstreams hold them whenever any is elsewhere.
      const sessions = others.some((other) => other.group.totalCount > 0);
      const where = new Map<string, number>();
      for (const other of others) where.set(other.stream, (where.get(other.stream) ?? 0) + (sessions ? other.group.totalCount : emptyWorkspaces(other.group)));
      const held = [...where].filter(([, amount]) => amount > 0);
      const total = held.reduce((sum, [, amount]) => sum + amount, 0);
      const more = sessions ? `${count(total, "more session")} of this project ${total === 1 ? "has" : "have"} no known epic` : `${count(total, "more workspace")} of this project without sessions`;
      const at = held.every(([place]) => place === stream) ? "" : held.length === 1 ? `, in ${held[0]![0]}` : `: ${held.map(([place, amount]) => `${amount} in ${place}`).join(", ")}`;
      details.set(group.key, [group.detail, `${more}${at}`].filter(Boolean).join(" · "));
    }
  }
  const names = projectNames(tree.flatMap((top) => top.kind === "workstream" ? [top.groups] : []), roots);
  return tree.map((top) => {
    if (top.kind !== "workstream") return top;
    const children = top.groups.map((group) => {
      const name = group.kind === "epic" ? names.get(projectOf(group) ?? "") : undefined;
      return { ...group, label: name ? `${group.label} (${name})` : group.label, detail: details.get(group.key) ?? group.detail };
    });
    // An Epic unknown row follows its project's last recognised row in the same workstream, not the end of it.
    const follows = new Map<string, OrganizationGroup[]>();
    for (const group of children) {
      if (group.kind !== "epic" || group.epic !== null) continue;
      const project = projectOf(group);
      const known = project ? children.findLast((other) => other.kind === "epic" && other.epic !== null && projectOf(other) === project) : undefined;
      if (known) follows.set(known.key, [...(follows.get(known.key) ?? []), group]);
    }
    const attached = new Set([...follows.values()].flat().map((group) => group.key));
    return { ...top, groups: children.filter((group) => !attached.has(group.key)).flatMap((group) => [group, ...(follows.get(group.key) ?? [])]) };
  });
}

/** The epic group's native project, from its sessions or, for workspaces without sessions, their native project. */
function projectOf(group: OrganizationGroup): string | null {
  const workspace = (current: OrganizationGroup): string | null => current.workspace?.projectKey ?? current.groups.map(workspace).find(Boolean) ?? null;
  return group.entries.find((entry) => entry.projectKey)?.projectKey ?? workspace(group);
}

function emptyWorkspaces(group: OrganizationGroup): number {
  return (group.kind === "workspace" && group.totalCount === 0 ? 1 : 0) + group.groups.reduce((sum, child) => sum + emptyWorkspaces(child), 0);
}

function contents(group: OrganizationGroup): string | null {
  const empty = emptyWorkspaces(group);
  return [group.totalCount ? `${count(group.totalCount, "session")} with no known epic` : null, empty ? `${count(empty, "workspace")} without sessions` : null].filter(Boolean).join(" · ") || null;
}

/**
 * Sibling rows labelled alike by different native projects. Projects that share a label in any workstream, directly
 * or through another such project, form one cohort named together, and every row of a project carries the same name.
 * Each named label is checked against every sibling's, so no two sibling rows read alike. The name is the last folder where the cohort's roots differ (separate checkouts
 * usually share the repository folder's name), but only below every root's home folder: a home folder, or one above
 * it, can name a person. Never a path or a native ID; when roots are missing, cannot tell the projects apart or would
 * reach a home folder, each project's position in the cohort names it instead.
 */
function projectNames(streams: readonly (readonly OrganizationGroup[])[], roots: ReadonlyMap<string, string>): Map<string, string> {
  const cohorts = new Map<string, Set<string>>();
  const first = new Map<string, number>();
  const join = (projects: readonly string[]) => {
    const cohort = new Set(projects.flatMap((project) => [...(cohorts.get(project) ?? [project])]));
    for (const project of cohort) cohorts.set(project, cohort);
  };
  // Every set of sibling rows that read alike once named, as the native projects behind them; a row without one
  // counts as a sibling but has nothing to name.
  const alike = (names: ReadonlyMap<string, string>) => streams.flatMap((groups) => {
    const byLabel = new Map<string, Set<string>>();
    for (const group of groups) {
      const project = group.kind === "epic" ? projectOf(group) : null;
      if (project && !first.has(project)) first.set(project, first.size);
      const name = project ? names.get(project) : undefined;
      const label = name ? `${group.label} (${name})` : group.label;
      byLabel.set(label, (byLabel.get(label) ?? new Set()).add(project ?? `row:${group.key}`));
    }
    return [...byLabel.values()].filter((rows) => rows.size > 1).map((rows) => [...rows].filter((row) => first.has(row)));
  });
  for (const projects of alike(new Map())) if (projects.length > 1) join(projects);
  // A name can still make a row read like a sibling outside its cohort (`repo (main)` beside a project named so). The
  // cohorts behind that row then count positions instead; if positions collide too, the projects are named together.
  const counted = new Set<string>();
  for (let round = 0; ; round += 1) {
    const names = new Map<string, string>();
    for (const cohort of new Set(cohorts.values())) {
      const projects = [...cohort].sort((left, right) => first.get(left)! - first.get(right)!);
      const paths = projects.map((project) => roots.get(project)?.split(/[\\/]/).filter(Boolean) ?? []);
      let shared = 0;
      while (paths.every((path) => path.length > shared + 1 && path.at(-1 - shared) === paths[0]!.at(-1 - shared))) shared += 1;
      const folders = paths.map((path) => path.length - 1 - shared > homeFolder(path) ? path.at(-1 - shared)! : "");
      const distinct = !projects.some((project) => counted.has(project)) && folders.every(Boolean) && new Set(folders).size === folders.length;
      projects.forEach((project, index) => names.set(project, distinct ? folders[index]! : `${index + 1} of ${projects.length}`));
    }
    let changed = false;
    for (const projects of alike(names)) {
      const named = projects.filter((project) => cohorts.has(project) && ![...cohorts.get(project)!].some((member) => counted.has(member)));
      if (named.length) named.forEach((project) => counted.add(project));
      else if (projects.some((project) => !cohorts.has(project) || cohorts.get(project) !== cohorts.get(projects[0]!))) {
        join(projects);
        counted.add(projects[0]!);
      } else continue;
      changed = true;
    }
    // ponytail: rows the pass cannot tell apart (one project's rows the tree labels alike, or rows without a native
    // project) stay as the tree labels them. Every round above counts a cohort or grows one, so it ends within two
    // rounds per project; the bound only keeps a render from ever spinning.
    if (!changed || round > 2 * first.size) return names;
  }
}

/** Index of the path's deepest home folder (`/Users/<name>`, `/home/<name>`, `C:\Users\<name>`, `/root`), or -1. */
function homeFolder(path: readonly string[]): number {
  return path.findLastIndex((segment, index) => index === 0 ? segment === "root" : /^(?:users|home|documents and settings)$/i.test(path[index - 1]!));
}

function count(value: number, word: string): string {
  return `${value} ${word}${value === 1 ? "" : "s"}`;
}

function unique(values: readonly string[]): string[] {
  return [...new Set(values)];
}
