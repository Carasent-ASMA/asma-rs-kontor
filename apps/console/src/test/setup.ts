import '@testing-library/jest-dom/vitest'
import { cleanup } from '@testing-library/react'
import { afterEach } from 'vitest'

// Testing Library only registers its own cleanup when Vitest is running with
// globals enabled, and this project keeps them off. Without this, one test's
// DOM is still on the page while the next one queries it — which turns every
// "found multiple elements" into a mystery about the component rather than
// about the harness.
afterEach(cleanup)
