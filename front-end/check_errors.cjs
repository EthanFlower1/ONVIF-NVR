const { execSync } = require('child_process');

try {
  const output = execSync('npx tsc --noEmit ./src/pages/playback.tsx', { encoding: 'utf-8' });
  console.log('No errors found');
} catch (error) {
  console.log('Errors found:');
  console.log(error.stdout);
}