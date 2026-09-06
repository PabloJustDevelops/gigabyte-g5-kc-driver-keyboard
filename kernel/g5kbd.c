// SPDX-License-Identifier: GPL-2.0-or-later
/*
 * g5kbd — Gigabyte G5 (Clevo-ODM) keyboard backlight kernel driver.
 *
 * Binds to the ACPI device CLV0001 (\_SB_.DCHU, the same device Windows'
 * AcpiBridge.sys and the TUXEDO/Clevo drivers use) and drives the single-zone
 * RGB keyboard backlight through the firmware's own _DSM handler, which wraps
 * the EC mailbox protocol that was reverse-engineered in this repo
 * (see ../docs/WINDOWS-RESEARCH.md).
 *
 *   _DSM UUID 93f224e4-fbdc-4bbf-add6-db71bdc0afad
 *   function   0x67  (CLEVO_CMD_SET_KB_RGB_LEDS)
 *   argument   packed 32-bit LED command, e.g.
 *                0xE007F001           master enable   (== FDAT 0x0C/0x3F, 0xC4)
 *                0xE0003001           master disable
 *                0xF0_000000 | b<<16 | r<<8 | g        zone-0 colour (B,R,G!)
 *                0xF4_000000 | level  brightness 0..255
 *
 * Exposed to userspace as the standard multicolor LED "rgb:kbd":
 *   /sys/class/leds/rgb:kbd/brightness   overall 0..255 (0 = off)
 *   /sys/class/leds/rgb:kbd/{red,green,blue}  colour intensities
 *   /sys/class/leds/rgb:kbd/color        convenience "RRGGBB" write
 *
 * The EC forgets the state on power loss, so a module-init default colour is
 * applied once at probe (module params color= / brightness=).
 */

#include <linux/acpi.h>
#include <linux/device.h>
#include <linux/dmi.h>
#include <linux/led-class-multicolor.h>
#include <linux/leds.h>
#include <linux/module.h>
#include <linux/moduleparam.h>
#include <linux/mutex.h>
#include <linux/slab.h>
#include <linux/uuid.h>

#define DRV_NAME "g5kbd"

/* \_SB_.DCHU — CLV0001 */
#define G5KBD_ACPI_HID "CLV0001"

#define G5KBD_CMD_SET_KB_RGB_LEDS	0x67
#define G5KBD_ARG_KB_ENABLE		0xE007F001UL
#define G5KBD_ARG_KB_DISABLE		0xE0003001UL
#define G5KBD_ARG_SUB_RGB_ZONE_0	0xF0000000UL
#define G5KBD_ARG_SUB_RGB_BRIGHTNESS	0xF4000000UL

static bool force;
module_param(force, bool, 0444);
MODULE_PARM_DESC(force, "bind even when DMI is not Gigabyte G5/G6/G7");

static uint color = 0x0000c8;	/* RRGGBB, firmware-ish default blue */
module_param(color, uint, 0444);
MODULE_PARM_DESC(color, "default colour to apply at probe, RRGGBB");

static uint brightness = 255;
module_param(brightness, uint, 0444);
MODULE_PARM_DESC(brightness, "default brightness to apply at probe, 0..255");

struct g5kbd_dev {
	acpi_handle handle;
	struct led_classdev_mc mc;
	struct mc_subled subled[3];	/* R, G, B */
	struct mutex lock;
};

/* _DSM UUID 93f224e4-fbdc-4bbf-add6-db71bdc0afad (little-endian wire order) */
static const guid_t g5kbd_dsm_uuid = {
	.b = { 0xe4, 0x24, 0xf2, 0x93, 0xdc, 0xfb, 0xbf, 0x4b,
	       0xad, 0xd6, 0xdb, 0x71, 0xbd, 0xc0, 0xaf, 0xad }
};

/*
 * Call the CLV0001 _DSM: function = cmd, argument = single integer (the
 * packed LED command), exactly like the TUXEDO clevo_acpi driver does.
 */
static int g5kbd_eval_dsm(acpi_handle handle, u64 func, u32 arg)
{
	union acpi_object arg4, pkg;
	union acpi_object *ret;
	acpi_status status;

	arg4.type = ACPI_TYPE_INTEGER;
	arg4.integer.value = arg;
	pkg.type = ACPI_TYPE_PACKAGE;
	pkg.package.count = 1;
	pkg.package.elements = &arg4;

	ret = acpi_evaluate_dsm(handle, &g5kbd_dsm_uuid, 0, func, &pkg);
	if (!ret)
		return -EIO;
	status = ret->type == ACPI_TYPE_INTEGER ? AE_OK : AE_ERROR;
	if (ACPI_SUCCESS(status) && (u64)ret->integer.value == 0xffffffffULL)
		status = AE_ERROR;	/* Clevo convention: 0xffffffff = nope */
	ACPI_FREE(ret);
	return ACPI_SUCCESS(status) ? 0 : -EIO;
}

/* Push current state (intensities + overall brightness) to the hardware. */
static int g5kbd_send(struct g5kbd_dev *dev)
{
	u32 rgb;
	int ret;

	mutex_lock(&dev->lock);

	if (dev->mc.led_cdev.brightness == 0) {
		ret = g5kbd_eval_dsm(dev->handle, G5KBD_CMD_SET_KB_RGB_LEDS,
				     G5KBD_ARG_KB_DISABLE);
		mutex_unlock(&dev->lock);
		return ret;
	}

	/* packed arg encodes blue|red|green in bits 23..0 (B,R,G byte order
	 * in the EC mailbox — verified live on a 2021 G5 KC, see docs) */
	rgb = G5KBD_ARG_SUB_RGB_ZONE_0
		| ((u32)dev->subled[2].intensity << 16)		/* blue  */
		| ((u32)dev->subled[0].intensity << 8)	/* red   */
		| (u32)dev->subled[1].intensity;	/* green */

	ret = g5kbd_eval_dsm(dev->handle, G5KBD_CMD_SET_KB_RGB_LEDS,
			     G5KBD_ARG_KB_ENABLE);
	if (!ret)
		ret = g5kbd_eval_dsm(dev->handle, G5KBD_CMD_SET_KB_RGB_LEDS, rgb);
	if (!ret)
		ret = g5kbd_eval_dsm(dev->handle, G5KBD_CMD_SET_KB_RGB_LEDS,
				     G5KBD_ARG_SUB_RGB_BRIGHTNESS
				     | dev->mc.led_cdev.brightness);
	mutex_unlock(&dev->lock);
	return ret;
}

static int g5kbd_brightness_set(struct led_classdev *led_cdev,
				enum led_brightness value)
{
	struct led_classdev_mc *mc = lcdev_to_mccdev(led_cdev);
	struct g5kbd_dev *dev = container_of(mc, struct g5kbd_dev, mc);

	return g5kbd_send(dev);
}

/* sysfs convenience attr: write "RRGGBB" to set all three channels. */
static ssize_t color_store(struct device *dev, struct device_attribute *attr,
			   const char *buf, size_t count)
{
	struct led_classdev *led_cdev = dev_get_drvdata(dev);
	struct led_classdev_mc *mc = lcdev_to_mccdev(led_cdev);
	struct g5kbd_dev *gdev = container_of(mc, struct g5kbd_dev, mc);
	unsigned long rgb;
	int ret;

	ret = kstrtoul(buf, 16, &rgb);
	if (ret || rgb > 0xffffff)
		return -EINVAL;

	mutex_lock(&gdev->lock);
	gdev->subled[0].intensity = (rgb >> 16) & 0xff;	/* red   */
	gdev->subled[1].intensity = (rgb >> 8) & 0xff;	/* green */
	gdev->subled[2].intensity = rgb & 0xff;		/* blue  */
	mutex_unlock(&gdev->lock);

	ret = g5kbd_send(gdev);
	if (ret)
		return ret;
	if (led_cdev->brightness == 0)	/* writing a colour turns it on */
		led_set_brightness(led_cdev, led_cdev->max_brightness);
	return count;
}

static DEVICE_ATTR_WO(color);

static struct attribute *g5kbd_attrs[] = {
	&dev_attr_color.attr,
	NULL,
};
ATTRIBUTE_GROUPS(g5kbd);

static int g5kbd_acpi_add(struct acpi_device *adev)
{
	struct g5kbd_dev *dev;
	int rc;

	/* Note: no _DSM feature pre-check here. Querying a function the
	 * firmware does not implement returns 0xffffffff (Clevo convention)
	 * which is NOT proof the cmd-0x67 LED mailbox is absent — on the G5 KC
	 * the firmware implements 0x67 but none of the "feature" queries.
	 * Register the LED and let the apply path surface any real errors. */

	dev = kzalloc(sizeof(*dev), GFP_KERNEL);
	if (!dev)
		return -ENOMEM;
	dev->handle = adev->handle;
	mutex_init(&dev->lock);

	dev->subled[0].color_index = LED_COLOR_ID_RED;
	dev->subled[1].color_index = LED_COLOR_ID_GREEN;
	dev->subled[2].color_index = LED_COLOR_ID_BLUE;
	dev->subled[0].intensity = (color >> 16) & 0xff;
	dev->subled[1].intensity = (color >> 8) & 0xff;
	dev->subled[2].intensity = color & 0xff;
	dev->subled[0].max_intensity = 255;
	dev->subled[1].max_intensity = 255;
	dev->subled[2].max_intensity = 255;

	dev->mc.num_colors = 3;
	dev->mc.subled_info = dev->subled;
	dev->mc.led_cdev.name = "rgb:kbd";
	dev->mc.led_cdev.max_brightness = 255;
	dev->mc.led_cdev.brightness_set_blocking = g5kbd_brightness_set;
	dev->mc.led_cdev.groups = g5kbd_groups;

	rc = devm_led_classdev_multicolor_register(&adev->dev, &dev->mc);
	if (rc)
		goto err;

	dev_set_drvdata(&adev->dev, dev);
	dev_info(&adev->dev, "Gigabyte G5 keyboard backlight registered\n");

	/* Apply the configured default (EC forgets everything at power-on). */
	if (brightness > 255)
		brightness = 255;
	dev->mc.led_cdev.brightness = brightness;
	g5kbd_send(dev);
	return 0;

err:
	mutex_destroy(&dev->lock);
	kfree(dev);
	return rc;
}

static void g5kbd_acpi_remove(struct acpi_device *adev)
{
	struct g5kbd_dev *dev = dev_get_drvdata(&adev->dev);

	/* Leave the keyboard in a sane state (master off). */
	if (dev) {
		g5kbd_eval_dsm(dev->handle, G5KBD_CMD_SET_KB_RGB_LEDS,
			       G5KBD_ARG_KB_DISABLE);
		mutex_destroy(&dev->lock);
	}
}

static const struct acpi_device_id g5kbd_acpi_ids[] = {
	{ G5KBD_ACPI_HID, 0 },
	{ "", 0 },
};
MODULE_DEVICE_TABLE(acpi, g5kbd_acpi_ids);

static struct acpi_driver g5kbd_acpi_driver = {
	.name = DRV_NAME,
	.class = DRV_NAME,
	.ids = g5kbd_acpi_ids,
	.ops = {
		.add = g5kbd_acpi_add,
		.remove = g5kbd_acpi_remove,
	},
};

/* Only drive Gigabyte G5/G6/G7 laptops (the Clevo _DSM also exists on many
 * other machines we have not tested). */
static bool g5kbd_dmi_ok(void)
{
	const char *vendor, *product;

	if (force)
		return true;
	vendor = dmi_get_system_info(DMI_SYS_VENDOR);
	product = dmi_get_system_info(DMI_PRODUCT_NAME);
	if (!vendor || !product)
		return false;
	if (strcmp(vendor, "GIGABYTE") != 0)
		return false;
	if (strncmp(product, "G5", 2) && strncmp(product, "G6", 2) &&
	    strncmp(product, "G7", 2))
		return false;
	return true;
}

static int __init g5kbd_init(void)
{
	if (!g5kbd_dmi_ok()) {
		pr_info(DRV_NAME ": not a Gigabyte G5/G6/G7 laptop, refusing "
			"to load (override with force=1)\n");
		return -ENODEV;
	}
	return acpi_bus_register_driver(&g5kbd_acpi_driver);
}

static void __exit g5kbd_exit(void)
{
	acpi_bus_unregister_driver(&g5kbd_acpi_driver);
}

module_init(g5kbd_init);
module_exit(g5kbd_exit);

MODULE_LICENSE("GPL");
MODULE_AUTHOR("PabloJustDevelops");
MODULE_DESCRIPTION("Gigabyte G5 (Clevo-ODM) keyboard backlight driver");
