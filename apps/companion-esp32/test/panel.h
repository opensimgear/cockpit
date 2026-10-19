/* Generated from a validated immutable Cockpit publication. Do not edit. */
#ifndef OSG_GENERATED_PANEL_H
#define OSG_GENERATED_PANEL_H
#include "companion.h"
static const osg_page osg_demo_pages[] = {
{ "main-page", "Flight instruments", 800, 480 },
{ "engine-page", "Engine instruments", 800, 480 }
};
static const osg_widget osg_demo_widgets[] = {
{ "master-widget", "master", "", "Master caution", "", "", "", OSG_BUTTON, 0, 0, { 0.025, 0.05, 0.45, 0.38 }, 0xdf3d, 0x1126, 0x8718, 0.0, 1.0, NULL, 0, 0 },
{ "caution-widget", "caution-channel", "caution", "Caution status", "", "", "", OSG_INDICATOR, 0, 0, { 0.525, 0.05, 0.45, 0.38 }, 0xdf3d, 0x1126, 0x8718, 0.0, 1.0, NULL, 0, 0 },
{ "radio-widget", "radio-channel", "radio", "Radio frequency", "COM ", " MHz", "", OSG_TEXT, 0, 0, { 0.025, 0.55, 0.45, 0.38 }, 0xdf3d, 0x1126, 0x8718, 0.0, 1.0, NULL, 0, 0 },
{ "speed-widget", "speed-channel", "speed", "Airspeed", "", "", "kt", OSG_NUMERIC, 0, 1, { 0.525, 0.55, 0.45, 0.38 }, 0xdf3d, 0x1126, 0x8718, 0.0, 600.0, NULL, 0, 0 },
{ "rpm-widget", "rpm-channel", "rpm", "Engine RPM", "", "", "%", OSG_GAUGE, 1, 0, { 0.1, 0.15, 0.8, 0.7 }, 0xdf3d, 0x1126, 0x8718, 0.0, 100.0, NULL, 0, 0 }
};
static const osg_panel osg_demo_panel = { "esp32-demo", "sha256:af25b3e73156cc526865807484694b70dacbd51306b58b9d4bc0786af322b093", osg_demo_pages, osg_demo_widgets, 2, 5 };
#endif
