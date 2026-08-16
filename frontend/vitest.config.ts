import { defineConfig } from 'vitest/config';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  test: {
    include: ['packages/**/*.test.ts', 'packages/**/*.test.tsx', 'scripts/**/*.test.ts'],
    environment: 'jsdom',
    setupFiles: ['./test/setup.ts'],
  },
});
