import { createContext, use, useEffect, useState, type ReactNode } from "react";

/** The project a page is about, for the navigation's "Issues (project)" link (upstream's `project` context). */
export type NavProject = { id: number; name: string };

const NavProjectContext = createContext<{
  project: NavProject | null;
  setProject: (project: NavProject | null) => void;
}>({ project: null, setProject: () => {} });

export function NavProjectProvider({ children }: { children: ReactNode }) {
  const [project, setProject] = useState<NavProject | null>(null);
  return <NavProjectContext value={{ project, setProject }}>{children}</NavProjectContext>;
}

export function useNavProject(): NavProject | null {
  return use(NavProjectContext).project;
}

/** Shows `project` in the navigation while the calling page is mounted. */
export function useSetNavProject(project: NavProject | null | undefined) {
  const { setProject } = use(NavProjectContext);
  const id = project?.id;
  const name = project?.name;
  useEffect(() => {
    setProject(id !== undefined && name !== undefined ? { id, name } : null);
    return () => setProject(null);
  }, [id, name, setProject]);
}
