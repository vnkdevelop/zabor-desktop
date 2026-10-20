export async function resolve(specifier, context, nextResolve) {
  if (specifier === 'electron') {
    return {
      format: 'module',
      shortCircuit: true,
      url: new URL('./electron-mock.mjs', import.meta.url).href
    };
  }
  if (
    (specifier === 'child_process' || specifier === 'node:child_process') &&
    (!context.parentURL || !context.parentURL.includes('child-process-mock.mjs'))
  ) {
    return {
      format: 'module',
      shortCircuit: true,
      url: new URL('./child-process-mock.mjs', import.meta.url).href
    };
  }
  try {
    return await nextResolve(specifier, context);
  } catch (err) {
    if (
      err &&
      err.code === 'ERR_MODULE_NOT_FOUND' &&
      (specifier.startsWith('./') || specifier.startsWith('../')) &&
      !specifier.endsWith('.ts') &&
      !specifier.endsWith('.js') &&
      !specifier.endsWith('.mjs')
    ) {
      for (const ext of ['.ts', '.js', '.mjs']) {
        try {
          return await nextResolve(specifier + ext, context);
        } catch {}
      }
    }
    throw err;
  }
}
