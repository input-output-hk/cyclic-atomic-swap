// -*- mode: Bluespec; -*-
/**
 * Homegrown spells
 */

module customSpells {

  import basicSpells.* from "basicSpells"

  /// Guard a set (of transitions) under a condition.
  // If it isn't met, return the empty set.
  pure def when(guard: bool, s: Set[a]): Set[a] = if (guard) s else Set()

  pure def toOption(guard: bool, x: a): Option[a] = if (guard) Some(x) else None

}
