// SPDX-License-Identifier: GPL-2.0-only
// Uncharted-Universal #408 probe: minimal Vulkan C smoke test against a static libvulkan-1
// (the lumen-emu Metal adapter). Authored here; links the adapter, does not contain it.
// Steps: instance, physical device, properties/features/extension report (including the ones
// gpu/ requires), device + queue, buffer fill + readback, image clear + copy readback.
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <vulkan/vulkan.h>

#define CHECK(x) do { VkResult r_ = (x); if (r_ != VK_SUCCESS) { printf("FAIL %s -> %d (line %d)\n", #x, (int)r_, __LINE__); return 1; } } while (0)

static uint32_t find_mem(VkPhysicalDevice pd, uint32_t bits, VkMemoryPropertyFlags want) {
    VkPhysicalDeviceMemoryProperties mp; vkGetPhysicalDeviceMemoryProperties(pd, &mp);
    for (uint32_t i = 0; i < mp.memoryTypeCount; ++i)
        if ((bits & (1u << i)) && (mp.memoryTypes[i].propertyFlags & want) == want) return i;
    return 0xffffffffu;
}

int main(void) {
    uint32_t iv = 0; vkEnumerateInstanceVersion(&iv);
    printf("instance version %u.%u.%u\n", VK_VERSION_MAJOR(iv), VK_VERSION_MINOR(iv), VK_VERSION_PATCH(iv));
    VkApplicationInfo app = { VK_STRUCTURE_TYPE_APPLICATION_INFO, 0, "lumen-probe", 1, "none", 1, VK_API_VERSION_1_2 };
    VkInstanceCreateInfo ici = { VK_STRUCTURE_TYPE_INSTANCE_CREATE_INFO, 0, 0, &app };
    VkInstance inst; CHECK(vkCreateInstance(&ici, NULL, &inst));
    uint32_t n = 0; CHECK(vkEnumeratePhysicalDevices(inst, &n, NULL));
    printf("physical devices: %u\n", n);
    if (!n) return 1;
    VkPhysicalDevice pds[8]; if (n > 8) n = 8; CHECK(vkEnumeratePhysicalDevices(inst, &n, pds));
    VkPhysicalDevice pd = pds[0];
    VkPhysicalDeviceProperties props; vkGetPhysicalDeviceProperties(pd, &props);
    printf("device: %s api %u.%u.%u driver 0x%x vendor 0x%x type %d\n", props.deviceName,
           VK_VERSION_MAJOR(props.apiVersion), VK_VERSION_MINOR(props.apiVersion), VK_VERSION_PATCH(props.apiVersion),
           props.driverVersion, props.vendorID, (int)props.deviceType);
    printf("limits: maxImageDim2D %u maxStorageBufferRange %u maxPushDescriptors n/a, maxBoundDescriptorSets %u\n",
           props.limits.maxImageDimension2D, props.limits.maxStorageBufferRange, props.limits.maxBoundDescriptorSets);

    uint32_t ne = 0; vkEnumerateDeviceExtensionProperties(pd, NULL, &ne, NULL);
    VkExtensionProperties* ex = calloc(ne, sizeof *ex); vkEnumerateDeviceExtensionProperties(pd, NULL, &ne, ex);
    printf("device extensions (%u):", ne); for (uint32_t i = 0; i < ne; ++i) printf(" %s", ex[i].extensionName); printf("\n");
    static const char* required[] = { "VK_KHR_swapchain", "VK_KHR_push_descriptor", "VK_EXT_vertex_attribute_divisor", "VK_EXT_robustness2" };
    for (unsigned r = 0; r < 4; ++r) { int has = 0; for (uint32_t i = 0; i < ne; ++i) has |= !strcmp(ex[i].extensionName, required[r]);
        printf("gpu/ HARD requirement %-34s %s\n", required[r], has ? "present" : "MISSING"); }

    VkPhysicalDeviceRobustness2FeaturesEXT rb = { VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_ROBUSTNESS_2_FEATURES_EXT };
    VkPhysicalDeviceVulkan13Features f13 = { VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_VULKAN_1_3_FEATURES, &rb };
    VkPhysicalDeviceVulkan12Features f12 = { VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_VULKAN_1_2_FEATURES, &f13 };
    VkPhysicalDeviceFeatures2 f2 = { VK_STRUCTURE_TYPE_PHYSICAL_DEVICE_FEATURES_2, &f12 };
    vkGetPhysicalDeviceFeatures2(pd, &f2);
    printf("features: sparseBinding %d sparseResidencyBuffer %d shaderFloat64 %d shaderInt64 %d geometryShader %d tessellation %d robustBufferAccess %d\n",
           f2.features.sparseBinding, f2.features.sparseResidencyBuffer, f2.features.shaderFloat64, f2.features.shaderInt64,
           f2.features.geometryShader, f2.features.tessellationShader, f2.features.robustBufferAccess);
    printf("v12: timelineSemaphore %d bufferDeviceAddress %d hostQueryReset %d scalarBlockLayout %d shaderFloat16 %d shaderInt8 %d drawIndirectCount %d separateDepthStencilLayouts %d shaderBufferInt64Atomics %d\n",
           f12.timelineSemaphore, f12.bufferDeviceAddress, f12.hostQueryReset, f12.scalarBlockLayout, f12.shaderFloat16,
           f12.shaderInt8, f12.drawIndirectCount, f12.separateDepthStencilLayouts, f12.shaderBufferInt64Atomics);
    printf("v13: dynamicRendering %d synchronization2 %d maintenance4 %d subgroupSizeControl %d shaderDemoteToHelperInvocation %d\n",
           f13.dynamicRendering, f13.synchronization2, f13.maintenance4, f13.subgroupSizeControl, f13.shaderDemoteToHelperInvocation);
    printf("robustness2: robustBufferAccess2 %d robustImageAccess2 %d nullDescriptor %d (struct %s)\n", rb.robustBufferAccess2,
           rb.robustImageAccess2, rb.nullDescriptor, "filled only if adapter handles the sType");

    uint32_t nq = 0; vkGetPhysicalDeviceQueueFamilyProperties(pd, &nq, NULL);
    VkQueueFamilyProperties qf[8]; if (nq > 8) nq = 8; vkGetPhysicalDeviceQueueFamilyProperties(pd, &nq, qf);
    printf("queue families: %u:", nq); for (uint32_t i = 0; i < nq; ++i) printf(" [%u flags 0x%x count %u]", i, qf[i].queueFlags, qf[i].queueCount); printf("\n");
    uint32_t qfi = 0xffffffffu; for (uint32_t i = 0; i < nq; ++i) if (qf[i].queueFlags & VK_QUEUE_GRAPHICS_BIT) { qfi = i; break; }
    if (qfi == 0xffffffffu) { printf("no graphics queue\n"); return 1; }

    float prio = 1.0f;
    VkDeviceQueueCreateInfo qci = { VK_STRUCTURE_TYPE_DEVICE_QUEUE_CREATE_INFO, 0, 0, qfi, 1, &prio };
    VkDeviceCreateInfo dci = { VK_STRUCTURE_TYPE_DEVICE_CREATE_INFO, 0, 0, 1, &qci };
    VkDevice dev; CHECK(vkCreateDevice(pd, &dci, NULL, &dev));
    VkQueue q; vkGetDeviceQueue(dev, qfi, 0, &q);
    printf("vkCreateDevice ok (no optional features requested)\n");

    // 1. buffer fill + readback
    const VkDeviceSize bsz = 4096;
    VkBufferCreateInfo bci = { VK_STRUCTURE_TYPE_BUFFER_CREATE_INFO, 0, 0, bsz, VK_BUFFER_USAGE_TRANSFER_DST_BIT | VK_BUFFER_USAGE_TRANSFER_SRC_BIT, VK_SHARING_MODE_EXCLUSIVE };
    VkBuffer buf; CHECK(vkCreateBuffer(dev, &bci, NULL, &buf));
    VkMemoryRequirements mr; vkGetBufferMemoryRequirements(dev, buf, &mr);
    uint32_t mt = find_mem(pd, mr.memoryTypeBits, VK_MEMORY_PROPERTY_HOST_VISIBLE_BIT | VK_MEMORY_PROPERTY_HOST_COHERENT_BIT);
    if (mt == 0xffffffffu) { printf("no host-visible+coherent memory type\n"); return 1; }
    VkMemoryAllocateInfo mai = { VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO, 0, mr.size, mt };
    VkDeviceMemory mem; CHECK(vkAllocateMemory(dev, &mai, NULL, &mem)); CHECK(vkBindBufferMemory(dev, buf, mem, 0));

    // 2. image clear + copy to a second buffer
    VkImageCreateInfo ii = { VK_STRUCTURE_TYPE_IMAGE_CREATE_INFO, 0, 0, VK_IMAGE_TYPE_2D, VK_FORMAT_R8G8B8A8_UNORM, {16, 16, 1}, 1, 1,
                             VK_SAMPLE_COUNT_1_BIT, VK_IMAGE_TILING_OPTIMAL, VK_IMAGE_USAGE_TRANSFER_DST_BIT | VK_IMAGE_USAGE_TRANSFER_SRC_BIT,
                             VK_SHARING_MODE_EXCLUSIVE, 0, 0, VK_IMAGE_LAYOUT_UNDEFINED };
    VkImage img; CHECK(vkCreateImage(dev, &ii, NULL, &img));
    VkMemoryRequirements imr; vkGetImageMemoryRequirements(dev, img, &imr);
    uint32_t imt = find_mem(pd, imr.memoryTypeBits, 0);
    VkMemoryAllocateInfo imai = { VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO, 0, imr.size, imt };
    VkDeviceMemory imem; CHECK(vkAllocateMemory(dev, &imai, NULL, &imem)); CHECK(vkBindImageMemory(dev, img, imem, 0));
    VkBuffer rb2; CHECK(vkCreateBuffer(dev, &bci, NULL, &rb2));
    VkMemoryRequirements mr2; vkGetBufferMemoryRequirements(dev, rb2, &mr2);
    VkMemoryAllocateInfo mai2 = { VK_STRUCTURE_TYPE_MEMORY_ALLOCATE_INFO, 0, mr2.size, mt };
    VkDeviceMemory mem2; CHECK(vkAllocateMemory(dev, &mai2, NULL, &mem2)); CHECK(vkBindBufferMemory(dev, rb2, mem2, 0));

    VkCommandPoolCreateInfo cpi = { VK_STRUCTURE_TYPE_COMMAND_POOL_CREATE_INFO, 0, 0, qfi };
    VkCommandPool pool; CHECK(vkCreateCommandPool(dev, &cpi, NULL, &pool));
    VkCommandBufferAllocateInfo cbi = { VK_STRUCTURE_TYPE_COMMAND_BUFFER_ALLOCATE_INFO, 0, pool, VK_COMMAND_BUFFER_LEVEL_PRIMARY, 1 };
    VkCommandBuffer cb; CHECK(vkAllocateCommandBuffers(dev, &cbi, &cb));
    VkCommandBufferBeginInfo bi = { VK_STRUCTURE_TYPE_COMMAND_BUFFER_BEGIN_INFO, 0, VK_COMMAND_BUFFER_USAGE_ONE_TIME_SUBMIT_BIT };
    CHECK(vkBeginCommandBuffer(cb, &bi));
    vkCmdFillBuffer(cb, buf, 0, bsz, 0xA5A5A5A5u);
    VkImageSubresourceRange sr = { VK_IMAGE_ASPECT_COLOR_BIT, 0, 1, 0, 1 };
    VkImageMemoryBarrier b1 = { VK_STRUCTURE_TYPE_IMAGE_MEMORY_BARRIER, 0, 0, VK_ACCESS_TRANSFER_WRITE_BIT, VK_IMAGE_LAYOUT_UNDEFINED, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, VK_QUEUE_FAMILY_IGNORED, VK_QUEUE_FAMILY_IGNORED, img, sr };
    vkCmdPipelineBarrier(cb, VK_PIPELINE_STAGE_TOP_OF_PIPE_BIT, VK_PIPELINE_STAGE_TRANSFER_BIT, 0, 0, NULL, 0, NULL, 1, &b1);
    VkClearColorValue cv; cv.float32[0] = 1.0f; cv.float32[1] = 0.5f; cv.float32[2] = 0.0f; cv.float32[3] = 1.0f;
    vkCmdClearColorImage(cb, img, VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL, &cv, 1, &sr);
    VkImageMemoryBarrier b2 = b1; b2.srcAccessMask = VK_ACCESS_TRANSFER_WRITE_BIT; b2.dstAccessMask = VK_ACCESS_TRANSFER_READ_BIT;
    b2.oldLayout = VK_IMAGE_LAYOUT_TRANSFER_DST_OPTIMAL; b2.newLayout = VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL;
    vkCmdPipelineBarrier(cb, VK_PIPELINE_STAGE_TRANSFER_BIT, VK_PIPELINE_STAGE_TRANSFER_BIT, 0, 0, NULL, 0, NULL, 1, &b2);
    VkBufferImageCopy reg = { 0, 0, 0, { VK_IMAGE_ASPECT_COLOR_BIT, 0, 0, 1 }, {0, 0, 0}, {16, 16, 1} };
    vkCmdCopyImageToBuffer(cb, img, VK_IMAGE_LAYOUT_TRANSFER_SRC_OPTIMAL, rb2, 1, &reg);
    CHECK(vkEndCommandBuffer(cb));
    VkFenceCreateInfo fci = { VK_STRUCTURE_TYPE_FENCE_CREATE_INFO };
    VkFence fence; CHECK(vkCreateFence(dev, &fci, NULL, &fence));
    VkSubmitInfo si = { VK_STRUCTURE_TYPE_SUBMIT_INFO, 0, 0, NULL, NULL, 1, &cb };
    CHECK(vkQueueSubmit(q, 1, &si, fence));
    CHECK(vkWaitForFences(dev, 1, &fence, VK_TRUE, 5000000000ull));

    void* p; CHECK(vkMapMemory(dev, mem, 0, bsz, 0, &p));
    int bad = 0; for (int i = 0; i < 1024; ++i) bad += ((uint32_t*)p)[i] != 0xA5A5A5A5u;
    vkUnmapMemory(dev, mem);
    printf("buffer fill readback: %s (%d bad words)\n", bad ? "FAIL" : "PASS", bad);
    CHECK(vkMapMemory(dev, mem2, 0, bsz, 0, &p));
    const uint8_t* px = (const uint8_t*)p;
    printf("image clear readback px0 = %u %u %u %u (expect ~255 128 0 255): %s\n", px[0], px[1], px[2], px[3],
           (px[0] == 255 && px[1] >= 127 && px[1] <= 128 && px[2] == 0 && px[3] == 255) ? "PASS" : "FAIL");
    vkUnmapMemory(dev, mem2);
    vkDeviceWaitIdle(dev);
    vkDestroyFence(dev, fence, NULL); vkDestroyCommandPool(dev, pool, NULL);
    vkDestroyBuffer(dev, buf, NULL); vkDestroyBuffer(dev, rb2, NULL); vkDestroyImage(dev, img, NULL);
    vkFreeMemory(dev, mem, NULL); vkFreeMemory(dev, mem2, NULL); vkFreeMemory(dev, imem, NULL);
    vkDestroyDevice(dev, NULL); vkDestroyInstance(inst, NULL);
    printf("done\n");
    return 0;
}
