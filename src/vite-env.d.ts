/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** Set to "1" by allbuild.sh -e to enable experimental features. */
  readonly VITE_EXPERIMENTAL?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
