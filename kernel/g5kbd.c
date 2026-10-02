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
#include <linux/version.h>

#define DRV_NAME "g5kbd"

/* \_SB_.DCHU — CLV0001 */
#define G5KBD_ACPI_HID "CLV0001"

#define G5KBD_CMD_SET_KB_RGB_LEDS	0x67
#define G5KBD_ARG_KB_ENABLE		0xE007F001UL
#define G5KBD_ARG_KB_DISABLE		0xE0003001UL
#define G5KBD_ARG_SUB_RGB_ZONE_0	0xF0000000UL
#define G5KBD_ARG_SUB_RGB_BRIGHTNESS	0xF4000000UL

/*
 * Fan control, decoded from the same DSDT (see ../docs/FAN-RESEARCH.md).
 *
 * Both fan commands go through the *same* SCMD dispatcher that serves the
 * keyboard command 0x67, so they take a plain integer argument:
 *
 *   0x68  set duty.  arg = 4 bytes, one per fan: [7:0]=fan1 (CPU),
 *         [15:8]=fan2 (GPU), [23:16]=fan3, [31:24]=fan4. The AML turns
 *         that into four separate mailbox writes FDAT=1..4, FBUF=duty,
 *         FCMD=0xC1 — one per fan, *every* call. There is no way to name a
 *         single fan: the other three bytes are written too, so a zero there
 *         stops the fan. That is why the sysfs side is a single `fan_duty`
 *         taking both duties, and not one attribute per fan.
 *   0x69  hand fans back to the firmware curve. arg is a bitmask; bit N
 *         releases fan N+1. Each set bit becomes FDAT=0xFF, FBUF=fan,
 *         FCMD=0xC1.
 *
 * There is deliberately no fan-*curve* command here. The firmware's curve
 * table (function 0x0E -> CC30 -> PK0E) only accepts two of its four points
 * from the OS, and each fan also carries three RPM set-point words that the
 * matching read command (0x0D) never returns. Writing that table therefore
 * means clobbering values we cannot read back, and whether the EC honours it
 * at all is unverified. `g5fan` shapes the ramp from userspace by driving the
 * duty above instead — see docs/FAN-RESEARCH.md.
 */
#define G5KBD_CMD_SET_FAN_DUTY		0x68
#define G5KBD_CMD_SET_FAN_AUTO		0x69

#define G5FAN_FAN_COUNT		2	/* CPU + GPU on this chassis */
#define G5FAN_ALL_FANS		((1u << G5FAN_FAN_COUNT) - 1)	/* 0x03 */

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
	/*
	 * True once a duty has been pinned. Fan telemetry is *not* mirrored
	 * here: the authoritative duty/RPM live in the EC, and the driver has
	 * no reliable way to read them back (see docs/FAN-RESEARCH.md), so
	 * rather than publish a stale copy we leave reading to g5fan.
	 */
	bool fan_manual;
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

/* Release both fans (or a subset) back to the firmware's own curve. */
static int g5fan_set_auto(struct g5kbd_dev *dev, unsigned int mask)
{
	if (!mask || (mask & ~G5FAN_ALL_FANS))
		return -EINVAL;
	return g5kbd_eval_dsm(dev->handle, G5KBD_CMD_SET_FAN_AUTO, mask);
}

/* EC registers holding the duty each fan is currently running at. */
#define G5FAN_DUTY_REG_CPU	0xCE
#define G5FAN_DUTY_REG_GPU	0xCF

static int g5fan_read_duty(int fan, u8 *duty)
{
	u8 reg;

	switch (fan) {
	case 0:
		reg = G5FAN_DUTY_REG_CPU;
		break;
	case 1:
		reg = G5FAN_DUTY_REG_GPU;
		break;
	default:
		return -EINVAL;
	}
	return ec_read(reg, duty);
}

/*
 * Pin the fans to fixed duties (0..255 each).
 *
 * Both are required, because that is the shape of the hardware: function
 * 0x68 assigns all four fans on every call. A read-modify-write built out of
 * `ec_read()` looks tempting and does not work — the EC's duty read-back lags
 * the mailbox write by more than the gap between two sysfs writes, so the
 * second write reads the *old* duty back and restores it over the first. That
 * is exactly how `g5fan doctor --write` came to report "35 % -> 35 % IGNORED"
 * for fan 1 while fan 2 took its new value. The callers all know both duties
 * anyway, so they hand over both and nothing is read here at all.
 */
static int g5fan_set_duties(struct g5kbd_dev *dev, const u8 *duty)
{
	u32 arg = 0;
	int i;

	for (i = 0; i < G5FAN_FAN_COUNT; i++)
		arg |= (u32)duty[i] << (i * 8);

	return g5kbd_eval_dsm(dev->handle, G5KBD_CMD_SET_FAN_DUTY, arg);
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

/* ------------------------------------------------------------------ */
/* Fan control                                                        */
/* ------------------------------------------------------------------ */

static struct g5kbd_dev *g5fan_from_dev(struct device *dev)
{
	return dev_get_drvdata(dev);
}

/* "auto" (firmware curve) or "manual" (we pin the duty). */
static ssize_t fan_mode_show(struct device *dev, struct device_attribute *attr,
			     char *buf)
{
	struct g5kbd_dev *g5 = g5fan_from_dev(dev);

	return sysfs_emit(buf, "%s\n", g5->fan_manual ? "manual" : "auto");
}

static ssize_t fan_mode_store(struct device *dev, struct device_attribute *attr,
			      const char *buf, size_t count)
{
	struct g5kbd_dev *g5 = g5fan_from_dev(dev);
	int ret;

	/*
	 * Only "auto" is accepted. "manual" deliberately is not: entering manual
	 * mode is what *writing a duty* does, and accepting the bare word would
	 * mean pinning both fans to duties we do not know (zero on a freshly
	 * loaded module), i.e. stopping them.
	 */
	if (!sysfs_streq(buf, "auto"))
		return -EINVAL;

	mutex_lock(&g5->lock);
	ret = g5fan_set_auto(g5, G5FAN_ALL_FANS);
	if (!ret)
		g5->fan_manual = false;
	mutex_unlock(&g5->lock);
	return ret ? ret : count;
}
static DEVICE_ATTR_RW(fan_mode);

/*
 * The duty of both fans at once, 0..255 each, as "cpu gpu".
 *
 * One attribute rather than one per fan because that is what the firmware
 * command is (see g5fan_set_duties): naming a single fan is not something
 * this EC can do, and pretending otherwise is how the CPU fan ends up at
 * zero. Every caller — turbo, manual, the curve daemon — computes both duty
 * values anyway, so nothing is lost.
 *
 * Reading answers from the EC, so it is what the fans are really doing
 * rather than a copy of the last write.
 */
static ssize_t fan_duty_show(struct device *dev, struct device_attribute *attr,
			     char *buf)
{
	struct g5kbd_dev *g5 = g5fan_from_dev(dev);
	u8 duty[G5FAN_FAN_COUNT];
	int i, ret = 0;

	mutex_lock(&g5->lock);
	for (i = 0; i < G5FAN_FAN_COUNT; i++) {
		ret = g5fan_read_duty(i, &duty[i]);
		if (ret)
			break;
	}
	mutex_unlock(&g5->lock);
	if (ret)
		return ret;

	return sysfs_emit(buf, "%u %u\n", duty[0], duty[1]);
}

static ssize_t fan_duty_store(struct device *dev, struct device_attribute *attr,
			      const char *buf, size_t count)
{
	struct g5kbd_dev *g5 = g5fan_from_dev(dev);
	unsigned int cpu, gpu;
	u8 duty[G5FAN_FAN_COUNT];
	int ret;

	if (sscanf(buf, "%u %u", &cpu, &gpu) != 2)
		return -EINVAL;
	if (cpu > 255 || gpu > 255)
		return -EINVAL;

	duty[0] = cpu;
	duty[1] = gpu;

	mutex_lock(&g5->lock);
	ret = g5fan_set_duties(g5, duty);
	if (!ret)
		g5->fan_manual = true;
	mutex_unlock(&g5->lock);
	return ret ? ret : count;
}
static DEVICE_ATTR_RW(fan_duty);

static struct attribute *g5kbd_attrs[] = {
	&dev_attr_color.attr,
	NULL,
};
ATTRIBUTE_GROUPS(g5kbd);

static struct attribute *g5fan_attrs[] = {
	&dev_attr_fan_mode.attr,
	&dev_attr_fan_duty.attr,
	NULL,
};
ATTRIBUTE_GROUPS(g5fan);

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
	/* Fans start on the firmware curve; g5fan pins a duty only on demand. */
	dev->fan_manual = false;
	dev->subled[1].color_index = LED_COLOR_ID_GREEN;
	dev->subled[2].color_index = LED_COLOR_ID_BLUE;
	dev->subled[0].intensity = (color >> 16) & 0xff;
	dev->subled[1].intensity = (color >> 8) & 0xff;
	dev->subled[2].intensity = color & 0xff;
#if LINUX_VERSION_CODE >= KERNEL_VERSION(7, 2, 0)
	/* struct mc_subled.max_intensity appeared in v7.2
	 * (led-class-multicolor.h). On older kernels the member does not
	 * exist and channels are implicitly 0..led_cdev.max_brightness
	 * (255 here) — the same full-scale behaviour we declare here. */
	dev->subled[0].max_intensity = 255;
	dev->subled[1].max_intensity = 255;
	dev->subled[2].max_intensity = 255;
#endif

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
	/*
	 * The fan attributes go on the ACPI platform device itself. That device
	 * is already registered with the driver core by the time probe() runs,
	 * so simply assigning adev->dev.groups here would be silently ignored —
	 * sysfs files are created during device_add(), long before this point.
	 * devm_device_add_group() is the supported way to add attributes to a
	 * device that is already live.
	 */
	rc = devm_device_add_group(&adev->dev, g5fan_groups[0]);
	if (rc) {
		dev_err(&adev->dev, "failed to add fan attributes: %d\n", rc);
		/*
		 * The ACPI device is already live, so take the driver data back
		 * out before freeing: a failed probe does not run remove(), and a
		 * dangling pointer there is a use-after-free waiting for the next
		 * sysfs access.
		 */
		dev_set_drvdata(&adev->dev, NULL);
		devm_led_classdev_unregister(&adev->dev, &dev->mc.led_cdev);
		mutex_destroy(&dev->lock);
		kfree(dev);
		return rc;
	}

	dev_info(&adev->dev, "Gigabyte G5 keyboard backlight + fan control registered\n");

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

	/* Leave the keyboard in a sane state (master off) and hand the fans
	 * back to the firmware curve — never leave a manual duty pinned. */
	if (dev) {
		g5kbd_eval_dsm(dev->handle, G5KBD_CMD_SET_KB_RGB_LEDS,
			       G5KBD_ARG_KB_DISABLE);
		g5fan_set_auto(dev, G5FAN_ALL_FANS);
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
MODULE_DESCRIPTION("Gigabyte G5 (Clevo-ODM) keyboard backlight and fan control driver");
