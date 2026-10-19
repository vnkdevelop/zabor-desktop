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
  return nextResolve(specifier, context);
}
