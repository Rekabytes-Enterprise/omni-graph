// One visual identity for each stored memory type, shared by graph, list and legend.
export const TYPES = {
  issue: { color: '#ff718b', icon: '!', label: 'Issue' },
  mistake: { color: '#ff9c78', icon: '×', label: 'Mistake' },
  fix: { color: '#64e6b3', icon: '✓', label: 'Fix' },
  lesson: { color: '#f6c56b', icon: '↗', label: 'Lesson' },
  decision: { color: '#b69aff', icon: '◆', label: 'Decision' },
  fact: { color: '#7dccff', icon: '•', label: 'Fact' },
  component: { color: '#82a5ff', icon: '◈', label: 'Component' },
  technology: { color: '#61d9e5', icon: '⌘', label: 'Technology' },
  constraint: { color: '#ec9dd0', icon: '⊣', label: 'Constraint' },
  procedure: { color: '#a1d5a8', icon: '≡', label: 'Procedure' },
  handoff: { color: '#a8b5d0', icon: '→', label: 'Handoff' },
  learning_goal: { color: '#d6e783', icon: '◎', label: 'Learning goal' },
  preference: { color: '#d3b5ed', icon: '♡', label: 'Preference' },
};
export function identity(kind) { return TYPES[kind] || { color: '#b3bfce', icon: '•', label: kind || 'Memory' }; }
