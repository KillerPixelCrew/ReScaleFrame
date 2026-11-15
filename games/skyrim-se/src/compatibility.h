// SPDX-License-Identifier: GPL-3.0-only
#pragma once
namespace rsf::skyrim {
// Diagnostic profile validation only. No private CS state is modified.
// Absence at these checkpoints is not a guarantee against arbitrary later loads.
// A known installed profile also refuses until a pre-initialization barrier exists.
bool compatibility_checkpoint(const char*& refusal) noexcept;
}
