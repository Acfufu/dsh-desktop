import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    include: ['packages/**/*.test.ts', 'packages/**/*.test.tsx', 'scripts/**/*.test.ts'],
    environment: 'jsdom',
    setupFiles: ['./test/setup.ts'],
  },
});
