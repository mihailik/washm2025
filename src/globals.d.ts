declare var isNode: boolean;
declare var isBrowser: boolean;

declare var instance: WebAssembly.Instance & {
  exports: WebAssembly.Instance['exports'] & {
    _continue(cookie: number): void;
  };
};

declare var wasiSnapshot: WASI['wasiImport'];
