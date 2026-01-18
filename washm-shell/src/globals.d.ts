declare var isNode: boolean;
declare var isBrowser: boolean;

declare var instance: WebAssembly.Instance & {
  exports: WebAssembly.Instance['exports'] & {
    _start(): void;
    _continue(cookie: number): void;
    memory: WebAssembly.Memory;
  };
};

declare var wasiSnapshot: WASI['wasiImport'];

/** special FD for bootstrap eval collection */
declare var FD_BOOTRAP: 20251219;

declare var mounted: Record<string, any>;

declare var washm_tty: {
  read(): Promise<Buffer | null>;
  enableRaw(): { ok: true } | { ok: false; error: string };
  disableRaw(): { ok: true } | { ok: false; error: string };
  size(): { ok: true, rows: number; cols: number } | { ok: false; error: string };
};

declare var washm_timer: {
  sleep(ms: number): Promise<void>;
};