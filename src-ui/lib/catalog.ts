export interface CatalogApp {
  id: string;
  name: string;
  repo: string;
  pitch: string;
  tagline: string;
  description: string;
  category: string;
  icon: string;
  screenshots: string[];
  platforms: Record<string, string>;
  linkProtocol: number[];
  minTools: string;
  docs: string;
  resident: boolean;
  homepage: string;
  installReason?: string | null;
}

export type BundleApps = '*' | string[];

export interface CatalogBundle {
  id: string;
  title: string;
  description: string;
  apps: BundleApps;
}

export interface Catalog {
  schema: number;
  updated: string;
  apps: CatalogApp[];
  tools: { id: string; name: string; repo: string; minTools: string; icon: string };
  bundles: CatalogBundle[];
}

export interface EngineUse {
  app: string;
  /** What the app uses the engine for. */
  for: string;
}

export interface Engine {
  id: string;
  name: string;
  homepage: string;
  usedBy: EngineUse[];
  linkDownload: boolean;
}

export interface CatalogData {
  catalog: Catalog;
  engines: { schema: number; engines: Engine[] };
  /** Why a refresh kept the saved copy, when it did. */
  offline?: string;
}
