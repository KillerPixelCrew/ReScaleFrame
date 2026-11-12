# Sharpening shader notices

The RCAS, depth-aware RCAS and depth-aware DAS sources in this directory are
ports of OptiScaler revision `2fff542e78e257a1e4ff5a1e21a69a166c78c335`
(2026-11-12), `OptiScaler/shaders/rcas/precompile/rcas.hlsl`,
`da_rcas_sharpen.hlsl` and `da_das_sharpen.hlsl`.
OptiScaler is GPL-3.0; RSF distributes these modified sources under GPL-3.0-only.
The complete GPL-3.0 license is in the repository root LICENSE.
Generated SPIR-V has the same provenance and license as its HLSL source.

RCAS credits RdenBlaauwen's RCAS-for-ReShade and AMD FidelityFX RCAS.
The RCAS-for-ReShade `RCAS.fx` includes the following MIT notice, checked at
https://github.com/RdenBlaauwen/RCAS-for-ReShade/blob/main/RCAS.fx on 2026-11-12.
The local FidelityFX `ffx_fsr3upscaler_rcas.h` also includes the MIT license
with Copyright (C) 2026 Advanced Micro Devices, Inc.

```text
// Copyright (C)2023 Advanced Micro Devices, Inc.
// 
// Permission is hereby granted, free of charge, to any person obtaining a copy 
// of this software and associated documentation files(the “Software”), to deal 
// in the Software without restriction, including without limitation the rights 
// to use, copy, modify, merge, publish, distribute, sublicense, and /or sell 
// copies of the Software, and to permit persons to whom the Software is 
// furnished to do so, subject to the following conditions :
// 
// The above copyright notice and this permission notice shall be included in 
// all copies or substantial portions of the Software.
// 
// THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR 
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, 
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT.IN NO EVENT SHALL THE 
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER 
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, 
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN 
// THE SOFTWARE.
```
