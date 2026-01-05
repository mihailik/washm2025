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
