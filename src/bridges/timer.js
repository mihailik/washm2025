(function() {
  globalThis.washm_timer = {
    sleep: (ms) => {
      return new Promise(resolve => setTimeout(() => resolve('done'), ms));
    }
  };
})();
