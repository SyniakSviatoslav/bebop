import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { loadSkills, findSkill, type Skill } from './skills.ts';

function skillDir(name: string, body: string): void {
  const dir = path.join(skillsDir, '.bebop', 'skills', name);
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(path.join(dir, 'SKILL.md'), body);
}

const skillsDir = path.join(os.tmpdir(), `bebop-skills-root-${Math.random().toString(36).slice(2)}`);
fs.mkdirSync(skillsDir, { recursive: true });
skillDir('review', '---\nname: review\ndescription: Run a focused code review pass.\n---\n# Review\nbody here\n');
skillDir('deploy', '---\nname: deploy\ndescription: Ship to staging then prod.\n---\n# Deploy\nbody\n');

// loadSkills reads from <cwd>/.bebop/skills; point cwd there.
const skills = loadSkills({ cwd: skillsDir, userSkills: '/no/home/skills' });

test('GREEN: loads SKILL.md with frontmatter from disk', () => {
  const names = skills.map((s) => s.name).sort();
  assert.deepEqual(names, ['deploy', 'review']);
  const review = skills.find((s) => s.name === 'review')!;
  assert.equal(review.description, 'Run a focused code review pass.');
  assert.ok(review.body.includes('body here'));
});

test('GREEN: findSkill exact name match', () => {
  const s = findSkill(skills, 'review');
  assert.equal(s?.name, 'review');
});

test('GREEN: findSkill substring over description', () => {
  const s = findSkill(skills, 'staging');
  assert.equal(s?.name, 'deploy');
});

test('GREEN: findSkill missing → undefined', () => {
  assert.equal(findSkill(skills, 'nonexistent-xyz'), undefined);
});

test('GREEN: missing dir → empty list (safe)', () => {
  const none = loadSkills({ cwd: '/no/such/cwd', userSkills: '/no/home' });
  assert.deepEqual(none, []);
});
