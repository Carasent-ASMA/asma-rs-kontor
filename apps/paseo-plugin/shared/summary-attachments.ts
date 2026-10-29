import { defineAttachmentSource, defineRpc } from "@getpaseo/plugin";
import { z } from "zod";

export const searchSummaryAttachments = defineRpc({
  name: "sessions.summary-resources",
  input: z.object({ query: z.string().max(200) }).strict(),
  output: z.object({ items: z.array(z.object({ id: z.string(), identifier: z.string(), title: z.string(), subtitle: z.string(), url: z.url(), text: z.string().min(1), resourceType: z.literal("kontor-summary-revision") })) }),
});
export const savedSummaryAttachments = defineAttachmentSource({ id: "saved-summaries", title: "Kontor saved summary", icon: "FileText",
  pickerTitle: "Choose a saved summary for this draft", searchPlaceholder: "Source title or exact summary ID and revision", search: searchSummaryAttachments });
export const summaryAttachmentIdentifier = (id: string, revision: number) => `${id}:r${revision}`;
export const SUMMARY_ATTACHMENT_STEPS = "Open the destination session and check its title. In its native Attach menu choose Kontor saved summary, then select the exact source and revision. Check the selected pill before sending. Close the picker without selecting to leave existing draft attachments unchanged. Selecting the same item again removes it.";
export const SUMMARY_ATTACHMENT_LIMIT = "Only your native picker selection adds the attachment. Kontor cannot insert remotely, verify an exact destination, or observe that selection. Nothing is sent automatically.";
