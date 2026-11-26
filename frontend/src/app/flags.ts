/**
 * Build-time feature flags. The internal design-system showcase exists in
 * development builds, or in a production build only when explicitly enabled
 * with VITE_ENABLE_DESIGN_SYSTEM=true or `vite build --mode showcase` (used
 * by the Playwright UI and visual suites).
 */
export const designSystemEnabled: boolean =
  import.meta.env.DEV ||
  import.meta.env.MODE === 'showcase' ||
  import.meta.env.VITE_ENABLE_DESIGN_SYSTEM === 'true';
