#ifndef OSG_CANDIDATE_H
#define OSG_CANDIDATE_H
#include "scene_bank.h"
/* Display-task candidate handoff. begin pins the candidate in drawing_panel;
 * finish releases that fence and only publishes a generation-matching result. */
const osg_panel *osg_network_candidate_begin(osg_scene_ticket *);
void osg_network_candidate_finish(osg_scene_ticket, bool, const osg_panel_source_diagnostic *);
#endif
