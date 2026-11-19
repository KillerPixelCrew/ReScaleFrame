// SPDX-License-Identifier: MIT
// The settings store's decimal text is the CRT's "%.9g", including its locale and its spellings
// of infinities and NaNs. Returns the character count, or -1 when the text does not fit.
#include <cstddef>
#include <cwchar>
extern "C" int rsf_settings_store_format_decimal(float value, wchar_t* text, std::size_t capacity) noexcept
{
    const int written = std::swprintf(text, capacity, L"%.9g", static_cast<double>(value));
    return written >= 0 && static_cast<std::size_t>(written) < capacity ? written : -1;
}
