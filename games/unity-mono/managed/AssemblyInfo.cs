// SPDX-License-Identifier: GPL-3.0-only
using System.Runtime.CompilerServices;
// The contract fixture inspects internal packet layouts and metadata preflight without making
// those implementation types part of the managed plugin's public API.
[assembly: InternalsVisibleTo("ReScaleFrame.Unity.ContractTests")]
