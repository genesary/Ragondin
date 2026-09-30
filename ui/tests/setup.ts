// Vitest's globals are off, so Testing Library cannot find an `afterEach` to
// register its own cleanup: without this, every rendered tree stays mounted
// into the next test. A no-op in a test that rendered nothing.
import { cleanup } from '@testing-library/react';
import { afterEach } from 'vitest';

afterEach(cleanup);
