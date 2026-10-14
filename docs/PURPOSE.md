# Why mc-rs exists

mc-rs is an experiment: to see how far a rebuild of Minecraft can go, and how much of the game
can be remade.

It starts from Minecraft 1.12.2, matched closely enough that a seed gives the same world as the
original game, and keeps adding the rest of the game piece by piece to find out where the limits
are.

## What it focuses on

- **Performance.** World generation, meshing and rendering are written in Rust to run fast and
  far. Render distance has no cap and distant terrain reaches past it, so the limits can be
  tested and pushed. [BENCHMARK.md](BENCHMARK.md) compares chunk generation speed against the
  original game.
- **Ease of play.** Getting into a world should take as few steps as possible. The launcher
  installs mc-rs, finds or downloads the 1.12.2 game files and updates itself, and friends join
  a hosted world from a browser link on a PC or a phone.
- **Faithfulness.** The same worlds, the same look and the same feel as 1.12.2, using the game's
  own textures, models and sounds from your copy of the game.
- **Playing together.** One PC hosts a shared world and friends build in it together.

## How it is built

mc-rs is developed heavily with AI coding agents. That is part of the experiment: seeing how much
of a game this size can be rebuilt that way. If you are against AI development, this project may
not be for you.
