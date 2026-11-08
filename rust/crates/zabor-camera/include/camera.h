#pragma once
#include "rust/cxx.h"
#include <cstdint>

namespace zabor::camera {
struct OwnerStatus;
struct Frame;
struct Device;
std::uint64_t begin(rust::Str device, bool enumerate);
OwnerStatus status(std::uint64_t owner);
Frame take_frame(std::uint64_t owner);
rust::Vec<Device> take_devices(std::uint64_t owner);
void cancel(std::uint64_t owner) noexcept;
void release(std::uint64_t owner) noexcept;
std::uint64_t begin_test(std::uint8_t mode, std::uint32_t delay_ms);
Frame test_copy_frame(rust::Slice<const std::uint8_t> data, std::uint32_t width,
                      std::uint32_t height, std::int32_t stride, std::size_t top);
}
